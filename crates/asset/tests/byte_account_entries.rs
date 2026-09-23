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
    // A git worktree carries no dump corpus; `LEGAIA_FUNCS_DIR` points one at
    // the main checkout's without a symlink the coverage gates would misread.
    if let Some(p) = std::env::var_os("LEGAIA_FUNCS_DIR").map(PathBuf::from) {
        return p.is_dir().then_some(p);
    }
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
    // The 15.9 MB monster archive: one LZS stream per monster id, in a fixed
    // 0x14000-byte slot the battle loader transfers whole. Every slot is
    // populated (186 blocks + 8 raw TIMs) and the file is an exact multiple of
    // the stride, so the rest of each slot is declared fill - an earlier note
    // here calling it "an unpopulated tail" was wrong on both counts.
    (867, 99.9, "monster_archive"),
    // `summon.dat` / `readef.DAT` - fixed 0x10800 slots, same treatment.
    (893, 99.9, "summon_readef"),
    (894, 99.9, "summon_readef"),
    // A standalone BGM SEQ behind one DATA_FIELD chunk header. Its class is
    // the generic overlay blob, so the walker comes from the bytes walking to
    // a terminator - the last entry on the disc whose sub-asset was found by
    // the magic sweep instead.
    (1062, 99.9, "stream"),
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
    // Boot `init.pak` - four publisher-logo TIMs. The floor is for the
    // no-dump-corpus run this harness makes: with `--funcs` the walker also
    // credits the overlay's code and the entry accounts nearly whole.
    (895, 92.0, "init_pak"),
    // The multi-bank VAB: 206 banks on the sector index its own head carries.
    (891, 100.0, "vab_multi_bank"),
    // Kingdom bundles (world map) - the three `scene_asset_table` carriers.
    (86, 95.0, "scene_asset_table"),
    (245, 95.0, "scene_asset_table"),
    (392, 95.0, "scene_asset_table"),
    // Two ordinary town scene bundles.
    (4, 95.0, "scene_asset_table"),
    (13, 95.0, "scene_asset_table"),
    // The same bundle shape below the scene-bundle detector's count window:
    // `dolk2` is count-4, `balden2` count-5, `other4` count-3, `other5`
    // count-1. Retail bounds the count nowhere, so all four walk identically.
    (69, 99.0, "descriptor_bundle"),
    (319, 99.0, "descriptor_bundle"),
    (1200, 99.0, "descriptor_bundle"),
    (1220, 99.0, "descriptor_bundle"),
    // The party pack `data\field\player.lzs`, count 3.
    (874, 99.0, "descriptor_bundle"),
    // Two entries in the same class that are NOT bundles - the class fits a
    // descriptor count instead of reading one. Both are offset packs, one
    // bare and one behind a DATA_FIELD chunk header.
    (872, 99.0, "descriptor_bundle"),
    (485, 99.0, "descriptor_bundle"),
    // The runtime `efect.dat` 2-pack: header, inline sprite atlas, and two
    // packs whose members are addressed by absolute file offset.
    (873, 100.0, "efect_pack"),
    // The two headerless 16bpp stills, claimed as the four bands their
    // consumer uploads.
    (1221, 100.0, "ringside_still"),
    (1222, 100.0, "ringside_still"),
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
/// The multi-bank VAB accounts entirely from lengths the container states.
///
/// The claim worth guarding is not the percentage - it is that **none** of it
/// comes from the magic sweep. This entry was the disc's largest `scan`-tier
/// figure: 96 % accounted, 0 % structural, every byte found by hunting `pBAV`
/// rather than by reading the 206-entry sector index in the head.
#[test]
fn the_multi_bank_vab_is_walked_not_swept() {
    let Some(dir) = extracted_root() else {
        eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
        return;
    };
    let Some(acc) = account_entry(&dir, 891, 1) else {
        eprintln!("[skip] entry 891 not extracted");
        return;
    };
    assert_invariants(&acc);
    assert_eq!(acc.walker.name(), "vab_multi_bank");
    assert_eq!(
        acc.accounted, acc.structural,
        "every claim on this entry must be structural, not a magic-sweep hit"
    );
    assert_eq!(acc.residue_bytes, 0, "the index table bounds every byte");

    let path = entry_path(&dir, 891).expect("entry 891");
    let bytes = std::fs::read(&path).expect("read entry 891");
    let bank = legaia_asset::vab_multi_bank::detect(&bytes).expect("detects");
    assert_eq!(bank.count, 206);
    assert_eq!(
        bank.banks.len(),
        206,
        "every bank resolves inside the buffer"
    );
    // The end sentinel is the archive's own sector count, the way a PROT TOC
    // entry's size is the gap to the next entry.
    let last = bank.banks.last().unwrap();
    assert_eq!(
        last.end_sector as usize * legaia_asset::vab_multi_bank::SECTOR,
        bytes.len(),
        "table[count] is the archive's sector count"
    );
    for b in &bank.banks {
        assert_eq!(
            &bytes[b.vab_offset()..b.vab_offset() + 4],
            b"pBAV",
            "bank {} does not start with a VAB",
            b.index
        );
        assert_eq!(
            b.header_len + b.body_len,
            b.fsize as usize,
            "bank {}: the two chunk payloads must sum to fsize",
            b.index
        );
        assert!(b.content_end() <= b.offset() + b.span());
    }
    eprintln!(
        "[ok]    891: {} banks, all claims structural",
        bank.banks.len()
    );
}

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
    // The shape loop above goes vacuous the moment the archive accounts whole,
    // so state that outcome rather than leaving a test that asserts nothing.
    assert_eq!(
        acc.residue_bytes, 0,
        "PROT 0867: every byte is either a slot's block or that slot's fill"
    );
    eprintln!(
        "[ok] PROT 0867 residue is padding only: {:?}",
        acc.by_shape
            .iter()
            .map(|s| (s.shape.as_str(), s.bytes))
            .collect::<Vec<_>>()
    );
}

/// The fixed-stride streaming slots: each slot's `pad` claim must be bounded by
/// the **stride** and made of fill.
///
/// This is the assertion that keeps `claim_slot_fill` from being a way to buy
/// percentage points. The loader transfers a whole slot, so the bytes past a
/// slot's content are declared slack - but only if the claim really does stop
/// at the declared boundary and really is fill. A walker that stopped early
/// inside live content would put a non-zero byte inside one of these claims,
/// and an off-by-one in the stride would end one somewhere other than a slot
/// edge. Both are checked against the raw file rather than against the parser.
#[test]
fn streaming_slot_fill_claims_are_bounded_fill() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    // (entry, slot stride) - the monster archive and the two battle side-band
    // streaming files.
    for (idx, stride) in [(867u32, 0x14000usize), (893, 0x10800), (894, 0x10800)] {
        let Some(path) = entry_path(&dir, idx) else {
            eprintln!("PROT {idx:04} not extracted - skipping");
            continue;
        };
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            bytes.len() % stride,
            0,
            "PROT {idx:04}: the extent must be an exact multiple of the stride, \
             or the slot boundary is not a declared bound"
        );
        // Re-derive the fill from the file: per slot, the maximal all-zero
        // suffix. Nothing here consults the parser.
        let slots = bytes.len() / stride;
        let mut fill_bytes = 0usize;
        for i in 0..slots {
            let slot = &bytes[i * stride..(i + 1) * stride];
            let mut j = slot.len();
            while j > 0 && slot[j - 1] == 0 {
                j -= 1;
            }
            assert!(j > 0, "PROT {idx:04}: slot {i} is entirely fill");
            fill_bytes += slot.len() - j;
        }
        let acc = account_entry(&dir, idx, 0).unwrap();
        assert_invariants(&acc);
        let claimed_pad = acc
            .by_owner
            .iter()
            .find(|o| o.owner == "pad")
            .map_or(0, |o| o.bytes);
        assert_eq!(
            claimed_pad, fill_bytes,
            "PROT {idx:04}: the `pad` owner must hold exactly the per-slot fill \
             re-derived from the file"
        );
        for r in &acc.residue {
            let slot_end = r.start.div_ceil(stride) * stride;
            assert!(
                r.end <= slot_end,
                "PROT {idx:04}: residue {:#x}..{:#x} straddles a slot boundary",
                r.start,
                r.end
            );
        }
        eprintln!(
            "[ok] PROT {idx:04}: {slots} slots, {fill_bytes} B fill, {:.2}% accounted, \
             {} B residue",
            acc.accounted_pct, acc.residue_bytes
        );
    }
}

/// The two stills are claimed as the four uploads their consumer performs, and
/// the arithmetic that makes that a claim rather than a guess holds on disc.
///
/// This used to assert the opposite - that both entries had no walker and
/// reported only a residue shape. That was a fair statement of the instrument
/// and a false one about the disc: the rectangle was already recovered from
/// `FUN_801F6B24`'s immediates, so what the entries lacked was a binding.
#[test]
fn the_stills_are_claimed_as_four_band_uploads() {
    use legaia_asset::ringside_still as still;
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let mut seen = 0usize;
    for idx in [still::PROT_INDEX_DEFAULT, still::PROT_INDEX_LOW_HP] {
        let Some(acc) = account_entry(&dir, idx, 1) else {
            continue;
        };
        seen += 1;
        assert_invariants(&acc);
        assert_eq!(acc.walker.name(), "ringside_still");
        assert_eq!(
            acc.size,
            still::ENTRY_BYTES,
            "PROT {idx:04}: the still is exactly 320 x 256 x 2 bytes"
        );
        let texture: usize = acc
            .by_owner
            .iter()
            .filter(|o| o.owner == "texture")
            .map(|o| o.bytes)
            .sum();
        assert_eq!(
            texture,
            still::ENTRY_BYTES,
            "PROT {idx:04}: every byte belongs to one of the four uploads"
        );
        assert_eq!(acc.residue_bytes, 0, "PROT {idx:04}: no residue");
        eprintln!("[ok] PROT {idx:04} {} = 4 band uploads", acc.label);
    }
    assert!(seen > 0, "neither still resolved");
}

/// Every pochi slot is the same 1927-byte fill file, and the 121 bytes above it
/// are not fill at all: each is byte-identical to some *other* entry's bytes at
/// the same file offset, which is the mastering buffer showing through.
///
/// The second half is what makes the filler's own accounting honest. Claiming
/// the whole sector as one thing would hide it; leaving the tail in the residue
/// would rank 266 slots of dev fill as work.
#[test]
fn pochi_slots_are_one_fill_file_plus_an_inherited_tail() {
    use legaia_asset::categorize::{POCHI_FILL_LEN, is_pochi_filler};
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
    for e in std::fs::read_dir(&dir).expect("read extracted/PROT") {
        let p = e.expect("dir entry").path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".BIN") {
            continue;
        }
        let Some(idx) = prot_index_from_name(name) else {
            continue;
        };
        entries.push((idx, std::fs::read(&p).expect("read entry")));
    }
    assert!(entries.len() > 1000, "extracted PROT looks truncated");

    let fillers: Vec<&(u32, Vec<u8>)> =
        entries.iter().filter(|(_, b)| is_pochi_filler(b)).collect();
    assert!(fillers.len() > 200, "pochi slots: {}", fillers.len());

    let reference = &fillers[0].1[..POCHI_FILL_LEN];
    let mut tail_donors = 0usize;
    for (idx, bytes) in &fillers {
        assert_eq!(bytes.len(), 2048, "PROT {idx:04}: a filler is one sector");
        assert_eq!(
            &bytes[..POCHI_FILL_LEN],
            reference,
            "PROT {idx:04}: the fill file differs from the others"
        );
        // The tail is another entry's bytes at the same offset. Any entry at
        // all, not a neighbour: the buffer is indexed by file offset and the
        // packer wrote whatever it last held.
        let tail = &bytes[POCHI_FILL_LEN..];
        if entries.iter().any(|(other, b)| {
            other != idx
                && b.len() >= 2048
                && !is_pochi_filler(b)
                && &b[POCHI_FILL_LEN..2048] == tail
        }) {
            tail_donors += 1;
        }
    }
    assert_eq!(
        tail_donors,
        fillers.len(),
        "every filler's tail must appear at the same offset in a non-filler entry"
    );
    eprintln!(
        "[ok] {} pochi slots: identical {POCHI_FILL_LEN}-byte fill file, \
         {tail_donors} inherited tails",
        fillers.len()
    );
}

/// A bundle's type-`0x14` `FLAG` descriptor holds the pochi fill file.
///
/// The dispatcher answers a `0x14` with `type << 8` and never touches the
/// payload (`docs/formats/asset-type.md`), so this slot's bytes are never read
/// at runtime - and every one of them on the disc is an LZS-compressed copy of
/// the same dev filler the 266 placeholder slots carry. That is what the slot
/// is: a reserved descriptor the authoring tool filled rather than dropped.
#[test]
fn the_flag_descriptor_slot_carries_the_pochi_fill_file() {
    use legaia_asset::categorize::{POCHI_FILL_LEN, is_pochi_filler};
    use legaia_asset::scene_asset_table::descriptor_bundle_walk;
    const FLAG_TYPE: u8 = 0x14;
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let mut checked = 0usize;
    for idx in [69u32, 121, 156, 200, 227, 319, 338, 372, 400, 647, 816] {
        let Some(path) = entry_path(&dir, idx) else {
            continue;
        };
        let bytes = std::fs::read(&path).expect("read entry");
        let descriptors = descriptor_bundle_walk(&bytes).expect("a descriptor bundle");
        let flag = descriptors
            .iter()
            .find(|d| d.type_byte == FLAG_TYPE)
            .unwrap_or_else(|| panic!("PROT {idx:04} has no FLAG descriptor"));
        assert_eq!(flag.size as usize, POCHI_FILL_LEN);
        let out = legaia_lzs::decompress(&bytes[flag.data_offset as usize..], flag.size as usize)
            .expect("the FLAG slot decodes");
        assert!(
            is_pochi_filler(&out),
            "PROT {idx:04}: the FLAG slot is not the pochi fill file"
        );
        checked += 1;
    }
    assert!(checked > 0, "no count-4/5 bundle resolved");
    eprintln!("[ok] {checked} FLAG descriptor slots carry the pochi fill file");
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

/// The overlay data-segment tables a parser here already reads are claimed
/// structurally, and each claim sits where no dumped function does.
///
/// Both halves matter. The first is the binding: a table with a `pub const`
/// offset in this workspace must not rank in the residue worklist beside a
/// format nobody has opened. The second is the guard on it - a pinned table
/// inside a dumped function's extent would mean the offset or the length is
/// wrong, and that overlap is invisible in the accounted total, because the
/// sink merges ranges before reporting and a merged range keeps no owner.
#[test]
fn pinned_overlay_tables_are_claimed_outside_every_code_extent() {
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    let dumps = legaia_asset::byte_account::read_dump_extents(&funcs).expect("read dumps");
    let map = legaia_asset::static_overlay::overlay_map();
    // Per entry: how many pinned-table rows, and their total bytes.
    let want: [(u32, usize, usize); 5] = [
        (898, 17, 2340),
        (899, 7, 1284),
        (975, 7, 446),
        (976, 5, 3124),
        (980, 8, 1928),
    ];
    let mut checked = 0usize;
    for (idx, n_rows, n_bytes) in want {
        let Some(path) = entry_path(&dir, idx) else {
            continue;
        };
        let bytes = std::fs::read(&path).expect("read entry");
        let rec = map.by_prot_index(idx).expect("overlay map row");
        let rows = legaia_asset::byte_account::pinned_overlay_tables(idx);
        assert_eq!(
            rows.len(),
            n_rows,
            "PROT {idx:04}: pinned-table row count ({} bytes)",
            rows.iter().map(|r| r.1).sum::<usize>()
        );
        assert_eq!(
            rows.iter().map(|r| r.1).sum::<usize>(),
            n_bytes,
            "PROT {idx:04}: pinned-table bytes"
        );
        // In bounds, and disjoint from each other.
        let mut spans: Vec<(usize, usize, &str)> =
            rows.iter().map(|r| (r.0, r.0 + r.1, r.3)).collect();
        spans.sort();
        for w in spans.windows(2) {
            assert!(
                w[0].1 <= w[1].0,
                "PROT {idx:04}: {} overlaps {}",
                w[0].2,
                w[1].2
            );
        }
        for &(_, b, what) in &spans {
            assert!(
                b <= bytes.len(),
                "PROT {idx:04}: {what} runs past the entry"
            );
        }
        // Disjoint from every dump extent the bytes confirm in this image.
        let hi = rec.base_va as u64 + bytes.len() as u64;
        for d in &dumps {
            if (d.entry_va as u64) < rec.base_va as u64 || (d.entry_va as u64) >= hi {
                continue;
            }
            if !matches!(
                legaia_asset::byte_account::attribute(d, &bytes, rec.base_va),
                legaia_asset::byte_account::Attribution::Confirmed
            ) {
                continue;
            }
            let cs = (d.entry_va - rec.base_va) as usize;
            let ce = (cs + d.bytes as usize).min(bytes.len());
            // The account refuses an extent that opens on the `$zero`-absolute
            // data signature - a table printed as code - so a pinned table
            // under one is the right claim, not an overlap.
            if legaia_asset::byte_account::zero_absolute_head(&bytes[cs..ce]) {
                continue;
            }
            for &(a, b, what) in &spans {
                assert!(
                    b <= cs || a >= ce,
                    "PROT {idx:04}: {what} overlaps confirmed FUN_{:08x}",
                    d.entry_va
                );
            }
        }
        // And the account really carries them.
        let opts = AccountOptions {
            prot_index: Some(idx),
            label: path.file_name().unwrap().to_str().unwrap().to_string(),
            funcs_dir: Some(funcs.clone()),
            depth: 0,
            ..Default::default()
        };
        let acc = account(&bytes, &opts);
        assert_invariants(&acc);
        let table_bytes: usize = acc
            .by_owner
            .iter()
            .filter(|o| o.owner != "code" && o.owner != "tim")
            .map(|o| o.bytes)
            .sum();
        // At least the pinned tables: switch tables, sized globals, formed
        // strings and call-argument records share these owners, so equality
        // stopped holding once those claims existed (it was only ever checked
        // where the dump corpus is present, which is why it went unnoticed).
        assert!(
            table_bytes >= n_bytes,
            "PROT {idx:04}: accounted non-code bytes {table_bytes} < pinned-table bytes {n_bytes}"
        );
        checked += 1;
    }
    assert!(checked >= 4, "expected the overlay entries on disc");
    eprintln!("[ok] {checked} overlay entries: pinned tables claimed, none inside a code extent");
}

/// Every player battle file accounts whole, and the one region that used to be
/// left over is claimed from bounds the container states rather than from the
/// shape of its bytes.
///
/// The gap sits between the descriptor table's terminator and the first
/// descriptor's data offset. Reading it as slack is only legitimate while it
/// is empty, so this asserts the bytes directly off the file instead of
/// through the parser - a walker that stopped early inside live content would
/// otherwise buy the difference.
#[test]
fn battle_data_pack_table_to_data_slack_is_empty() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let mut checked = 0usize;
    for idx in [863u32, 864, 865, 866] {
        let Some(path) = entry_path(&dir, idx) else {
            continue;
        };
        let bytes = std::fs::read(&path).expect("read entry");
        let pack = legaia_asset::battle_data_pack::detect(&bytes).expect("battle data pack");
        let table_end = pack.table_offset + (pack.records.len() + 1) * 12;
        let data_start = pack
            .records
            .iter()
            .map(|r| pack.data_base + r.data_offset as usize)
            .min()
            .expect("at least one descriptor");
        assert!(data_start > table_end, "PROT {idx:04}: no gap to claim");
        assert!(
            bytes[table_end..data_start].iter().all(|&b| b == 0),
            "PROT {idx:04}: the table-to-data gap is not empty"
        );
        let acc = account_entry(&dir, idx, 0).expect("account");
        assert_invariants(&acc);
        // What is left is inter-record alignment, nothing a shape test names.
        assert!(
            acc.residue_bytes < 16,
            "PROT {idx:04}: {} bytes of residue left",
            acc.residue_bytes
        );
        checked += 1;
    }
    assert!(checked >= 3, "expected the player battle files on disc");
    eprintln!("[ok] {checked} battle data packs account whole");
}

/// The uninitialised-data-region claim is exactly one maximal all-zero run,
/// and only where the image's own code addresses it.
///
/// Three things have to hold together or the rule is a way to buy percentage
/// points: every claim's bytes are all zero (so it can never grow into live
/// content), its bounds are the zero run's own (so it is not a tuned window),
/// and the runs the rule refuses stay in the residue. The STR overlay carries
/// all three cases in one entry - a 131172-byte region addressed at many
/// sites, a post-blob tail addressed at none, and a data-segment hole
/// addressed at none.
#[test]
fn uninitialised_data_claims_are_whole_zero_runs_the_image_addresses() {
    use legaia_asset::byte_account::{BSS_RUN_MIN, formed_addresses, zero_runs};
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    let map = legaia_asset::static_overlay::overlay_map();
    let mut checked = 0usize;
    for idx in [970u32, 899, 980, 975] {
        let (Some(path), Some(rec)) = (
            entry_path(&dir, idx),
            map.overlays.iter().find(|r| r.prot_index == idx),
        ) else {
            continue;
        };
        let bytes = std::fs::read(&path).expect("read entry");
        let opts = AccountOptions {
            prot_index: Some(idx),
            label: format!("{idx:04}"),
            funcs_dir: Some(funcs.clone()),
            depth: 0,
            keep_claims: true,
            ..Default::default()
        };
        let acc = account(&bytes, &opts);
        assert_invariants(&acc);
        let runs = zero_runs(&bytes, BSS_RUN_MIN);
        let formed = formed_addresses(&bytes, rec.base_va);
        let claims: Vec<_> = acc
            .claims
            .iter()
            .filter(|c| c.detail.starts_with("uninitialised data region"))
            .collect();
        assert!(
            !claims.is_empty(),
            "PROT {idx:04}: no uninitialised-data claim"
        );
        for c in &claims {
            assert!(
                bytes[c.start..c.end].iter().all(|&b| b == 0),
                "PROT {idx:04}: claim {:#x}..{:#x} is not all zero",
                c.start,
                c.end
            );
            assert!(
                runs.contains(&(c.start, c.end)),
                "PROT {idx:04}: claim {:#x}..{:#x} is not a maximal zero run",
                c.start,
                c.end
            );
            let (lo, hi) = (rec.base_va + c.start as u32, rec.base_va + c.end as u32);
            assert!(
                formed.iter().any(|(_, t)| *t >= lo && *t < hi),
                "PROT {idx:04}: claim {:#x}..{:#x} is addressed by nothing",
                c.start,
                c.end
            );
        }
        checked += 1;
        eprintln!(
            "[ok] PROT {idx:04}: {} uninitialised-data claim(s) of {} zero run(s), \
             structural {:.1}%",
            claims.len(),
            runs.len(),
            acc.structural_pct
        );
    }
    assert!(checked >= 3, "expected the mapped overlays on disc");

    // The refused runs: entry 0970's post-blob tail and its data-segment hole
    // carry no formed address, and both stay out of the claim set.
    let Some(path) = entry_path(&dir, 970) else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read entry");
    let base = map
        .overlays
        .iter()
        .find(|r| r.prot_index == 970)
        .expect("0970 row")
        .base_va;
    let formed = formed_addresses(&bytes, base);
    let runs = zero_runs(&bytes, BSS_RUN_MIN);
    let addressed = runs
        .iter()
        .filter(|(s, e)| {
            let (lo, hi) = (base + *s as u32, base + *e as u32);
            formed.iter().any(|(_, t)| *t >= lo && *t < hi)
        })
        .count();
    assert_eq!(
        addressed, 1,
        "PROT 0970: exactly one of its zero runs is addressed"
    );
    eprintln!(
        "[ok] PROT 0970: 1 of {} zero runs is addressed by the image's own code",
        runs.len()
    );
}

/// The STR overlay's dispatch table, its movie paths and its VLC blob are all
/// claimed, and the blob's extent is the one the unpacker's walk consumed.
#[test]
fn the_str_overlay_data_segment_is_claimed_structurally() {
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    let Some(path) = entry_path(&dir, 970) else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read entry");
    let opts = AccountOptions {
        prot_index: Some(970),
        label: "0970".into(),
        funcs_dir: Some(funcs),
        depth: 0,
        keep_claims: true,
        ..Default::default()
    };
    let acc = account(&bytes, &opts);
    assert_invariants(&acc);

    use legaia_asset::fmv_dispatch as fmv;
    use legaia_mdec::strv2_table as vlc;
    let base = fmv::STR_OVERLAY_BASE_VA;
    let src = (vlc::STRV2_PACKED_VA - base) as usize;
    let (table, consumed) = vlc::unpack_lz_tracked(&bytes[src..]).expect("the blob terminates");
    assert_eq!(table.len(), vlc::STRV2_TABLE_BYTES, "decoded table length");
    let blob = acc
        .claims
        .iter()
        .find(|c| c.detail.starts_with("STRv2 VLC table source"))
        .expect("the VLC blob is claimed");
    assert_eq!((blob.start, blob.end - blob.start), (src, consumed));
    // The blob is the last content in the entry: what follows is inside one
    // sector and is claimed as slack, not left as residue.
    assert!(bytes[blob.end..].iter().all(|&b| b == 0));
    assert!(bytes.len() - blob.end < 2048);

    let paths = acc
        .claims
        .iter()
        .filter(|c| c.detail.starts_with("movie path for fmv_id"))
        .count();
    assert_eq!(paths, fmv::FMV_SLOT_COUNT, "one path string per slot");
    assert!(
        acc.claims
            .iter()
            .any(|c| c.detail.starts_with("FMV dispatch table")),
        "the dispatch table is claimed"
    );
    eprintln!(
        "[ok] PROT 0970: VLC blob {consumed} bytes -> {} table bytes, {paths} movie paths, \
         structural {:.1}%",
        table.len(),
        acc.structural_pct
    );
}

/// The `OTHER3` dev module's roster is one 81-record table on a `0x84` stride,
/// and claiming it at the stride leaves the entry near whole.
#[test]
fn the_other3_roster_accounts_the_dev_module() {
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    let Some(path) = entry_path(&dir, 974) else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read entry");
    use legaia_asset::other3_roster as roster;
    let recs = roster::records(&bytes).expect("the roster parses");
    assert_eq!(recs.len(), roster::RECORD_COUNT);
    // A roster of 81 identical records would pass the lead-byte guard and mean
    // nothing; the labels differ, and most of each record is its NUL padding.
    let distinct: std::collections::BTreeSet<&[u8]> = recs.iter().copied().collect();
    assert!(distinct.len() > 60, "{} distinct labels", distinct.len());

    let opts = AccountOptions {
        prot_index: Some(974),
        label: "0974".into(),
        funcs_dir: Some(funcs),
        depth: 0,
        ..Default::default()
    };
    let acc = account(&bytes, &opts);
    assert_invariants(&acc);
    assert!(
        acc.structural_pct > 95.0,
        "PROT 0974 structural {:.1}%",
        acc.structural_pct
    );
    eprintln!(
        "[ok] PROT 0974: {} roster labels, structural {:.1}%",
        recs.len(),
        acc.structural_pct
    );
}

/// PROT `0975`'s trailing `plausible_mips` residue is PROT `0972`'s code at
/// the same file offset - an inherited tail, not this image's own bytes.
///
/// The run has no prologue, no `jr ra` and no caller, which invites reading it
/// as a jump-table body or the interior of a neighbour. Byte equality settles
/// it, and this test is the standing form of that measurement: byte accounting
/// does not cut inherited tails the way `disc-coverage.py` does, so a
/// `plausible_mips` run in an overlay entry is only un-dumped code once it has
/// been checked against the other entries at the same offset.
#[test]
fn the_slot_machine_tail_is_the_fishing_overlays_code() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let (Some(a), Some(b)) = (entry_path(&dir, 975), entry_path(&dir, 972)) else {
        return;
    };
    let slot = std::fs::read(a).expect("read 0975");
    let fishing = std::fs::read(b).expect("read 0972");
    const TAIL: usize = 0x5920;
    assert!(fishing.len() > slot.len());
    assert_eq!(
        &slot[TAIL..],
        &fishing[TAIL..slot.len()],
        "0975's tail is not 0972's bytes at the same offset"
    );
    eprintln!(
        "[ok] PROT 0975 file {TAIL:#x}..{:#x} ({} bytes) == PROT 0972 at the same offset",
        slot.len(),
        slot.len() - TAIL
    );
}

/// Records a call receives are claimed off the call, through the three staging
/// shapes retail uses, and a label-credited dump that opens on fill credits
/// nothing.
#[test]
fn call_argument_records_are_claimed_and_fill_headed_labels_refused() {
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    let run = |idx: u32| -> Option<Account> {
        let path = entry_path(&dir, idx)?;
        let bytes = std::fs::read(&path).ok()?;
        let opts = AccountOptions {
            prot_index: Some(idx),
            label: path.file_name()?.to_str()?.to_string(),
            funcs_dir: Some(funcs.clone()),
            prot_dir: Some(dir.clone()),
            depth: 0,
            keep_claims: true,
            ..Default::default()
        };
        Some(account(&bytes, &opts))
    };
    let note = |acc: &Account, needle: &str| {
        acc.notes
            .iter()
            .find(|n| n.contains(needle))
            .cloned()
            .unwrap_or_default()
    };
    // PROT 0957 stages records in a saved register (`move a2,s0`) and in the
    // delay slots of `switch` arms that jump to one shared call.
    let Some(a957) = run(957) else { return };
    assert_invariants(&a957);
    let n = note(&a957, "FUN_80050ed4 call(s)");
    assert!(n.contains("18 claimed"), "0957: {n}");
    // PROT 0980 hands every dancer effect through a local wrapper that
    // forwards its `$a3` as the spawn's `$a2`.
    let Some(a980) = run(980) else { return };
    let n = note(&a980, "FUN_801d3fd0 call(s)");
    assert!(n.contains("8 claimed"), "0980: {n}");
    // PROT 0897: nine effect scripts and twenty actor templates.
    let Some(a897) = run(897) else { return };
    assert!(note(&a897, "FUN_80021b04 call(s)").contains("9 claimed"));
    assert!(note(&a897, "FUN_80020de0 call(s)").contains("20 claimed"));
    // PROT 0976: the fill-headed `FUN_801d84b4` extent is not code.
    let Some(a976) = run(976) else { return };
    assert!(
        !a976
            .claims
            .iter()
            .any(|c| c.detail.starts_with("FUN_801d84b4")),
        "0976 still credits the fill-headed FUN_801d84b4 extent"
    );
    eprintln!("[ok] call-argument records on 0957 / 0980 / 0897; 0976 fill-headed label refused");
}
