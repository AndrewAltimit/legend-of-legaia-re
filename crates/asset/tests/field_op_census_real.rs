//! The disc-wide field-VM opcode census, over the real `extracted/PROT`.
//!
//! What this pins is the census's *answers* for the rare arms, because those
//! answers are what reach rows and thread verdicts are written from. A silent
//! change in the decoder's sub-op widths, in a partition's first-opcode
//! formula, or in the carrier set moves them without moving anything a unit
//! test can see.
//!
//! Skips and passes without `extracted/` - the usual gating, so CI needs no
//! Sony bytes.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::field_disasm::{OpCensus, OpKey, clean_hit_offsets, man_script_spans, tally_man};

fn prot_dir() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

fn key(opcode: u8, sub: Option<u8>) -> OpKey {
    OpKey { opcode, sub }
}

/// Walk every MAN payload every PROT entry carries: the scene bundle's
/// `type 0x03` descriptors (LZS-packed) and every `type 0x03` DATA_FIELD
/// stream chunk. Same set the CLI walks, minus the event-script carriers,
/// which carry none of the arms this test pins.
fn man_payloads(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = legaia_asset::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start >= buf.len() {
                continue;
            }
            if let Ok((decoded, _)) = legaia_lzs::decompress_tracked(&buf[start..], d.size as usize)
            {
                out.push(decoded);
            }
        }
    }
    if let Ok(report) = legaia_asset::parse_streaming(buf, 4096) {
        for chunk in &report.chunks {
            if chunk.type_byte != 0x03 {
                continue;
            }
            let start = chunk.header_offset + 4;
            if let Some(payload) = buf.get(start..start.saturating_add(chunk.size as usize))
                && legaia_asset::man_section::parse(payload).is_ok()
            {
                out.push(payload.to_vec());
            }
        }
    }
    out
}

struct Census {
    census: OpCensus,
    /// Entry index of every carrier with at least one clean hit, per key.
    carriers: std::collections::BTreeMap<OpKey, BTreeSet<u32>>,
}

fn run() -> Option<Census> {
    let dir = prot_dir()?;
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    let mut out = Census {
        census: OpCensus::default(),
        carriers: Default::default(),
    };
    let watched = [
        key(0x4C, Some(0xEA)),
        key(0x4C, Some(0x52)),
        key(0x4C, Some(0xCF)),
        key(0x4C, Some(0xC4)),
        key(0x4C, Some(0x3E)),
    ];
    for path in &entries {
        let Ok(buf) = std::fs::read(path) else {
            continue;
        };
        let idx: u32 = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split_once('_'))
            .and_then(|(n, _)| n.parse().ok())
            .unwrap_or(u32::MAX);
        let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
        for man in man_payloads(&buf) {
            if !seen.insert(man.clone()) {
                continue;
            }
            let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
                continue;
            };
            tally_man(&man_file, &man, &mut out.census, None);
            for (_, _, start, pc0, len) in man_script_spans(&man_file, &man) {
                let body = &man[start..start + len];
                for k in watched {
                    if !clean_hit_offsets(body, pc0, k).is_empty() {
                        out.carriers.entry(k).or_default().insert(idx);
                    }
                }
            }
        }
    }
    Some(out)
}

/// The two arms whose runtime-reach rows turned on "does a carrier exist".
/// Both do; the exact counts are the finding, and both moved when the walk
/// learned to cross a record's text segments.
#[test]
fn the_scripted_game_over_and_take_item_arms_have_carriers() {
    let Some(c) = run() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };

    // One per kingdom world-map bundle (`map01` / `map02` / `map03`). The
    // `map01` and `map02` sites were once read as residue: the walk reached
    // them only after the decoder learned to step over bare text segments,
    // pickers and the partition-0 record header.
    let game_over = key(0x4C, Some(0xEA));
    assert_eq!(
        c.census.clean_count(game_over),
        3,
        "[4C EA] has one coherent occurrence per kingdom world-map bundle"
    );
    assert_eq!(
        c.carriers.get(&game_over).map(|s| s.len()),
        Some(3),
        "[4C EA]'s three carriers are three entries"
    );
    for entry in [86, 245, 392] {
        assert!(
            c.carriers[&game_over].contains(&entry),
            "[4C EA]'s carriers are the map01/map02/map03 bundles (missing {entry})"
        );
    }

    // TAKE_ITEM is the chest script's item consume, one line into its record
    // ("...Treasure Chest!", Nop, `4C 52 <item>`, fades) - so a walk that
    // ended at the first text segment saw three sites; crossing text it sees
    // them all.
    let take_item = key(0x4C, Some(0x52));
    assert_eq!(c.census.clean_count(take_item), 70, "[4C 52] sites");
    assert_eq!(
        c.carriers.get(&take_item).map(|s| s.len()),
        Some(25),
        "[4C 52] carriers"
    );
    for entry in [166, 208, 339, 183] {
        assert!(
            c.carriers[&take_item].contains(&entry),
            "[4C 52] carrier {entry} (geremi / ropeway / ropeway2 / balden) missing"
        );
    }
}

/// The script camera-focus override, whose one-scene concentration is the
/// reason it reads as scene-specific framing rather than a general mechanism.
#[test]
fn the_camera_focus_override_is_confined_to_one_scene() {
    let Some(c) = run() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };
    let k = key(0x4C, Some(0xCF));
    assert_eq!(c.census.clean_count(k), 50, "[4C CF] clean occurrences");
    assert_eq!(
        c.carriers.get(&k).map(|s| s.len()),
        Some(1),
        "[4C CF] is carried by exactly one entry"
    );
    assert!(
        c.carriers[&k].contains(&435),
        "[4C CF]'s carrier is the uru bundle at extraction entry 435"
    );
}

/// Cross-check against the camera lane's independent per-arm instrument,
/// which counts sites with no coherence gate. Its figures are this census's
/// `total`, and for the two rare arms its scene counts are the `clean` ones
/// too - the agreement is what says the coherence split did not invent a
/// number.
#[test]
fn the_camera_arm_counts_agree_with_the_per_arm_instrument() {
    let Some(c) = run() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };
    let total = |k: OpKey| c.census.total.get(&k).copied().unwrap_or(0);
    assert_eq!(total(key(0x4C, Some(0x38))), 258, "[4C 38] sites");
    assert_eq!(total(key(0x4C, Some(0x39))), 323, "[4C 39] sites");
    assert_eq!(total(key(0x4C, Some(0xC4))), 44, "[4C C4] sites");
    assert_eq!(total(key(0x4C, Some(0x3E))), 5, "[4C 3E] sites");

    for (k, scenes) in [(key(0x4C, Some(0xC4)), 2), (key(0x4C, Some(0x3E)), 2)] {
        assert_eq!(
            c.carriers.get(&k).map(|s| s.len()),
            Some(scenes),
            "{k} carrier count"
        );
    }
}

/// The walk's own health. Not a quality target - roughly half of all records
/// desync somewhere, because most of them are text-heavy - but a floor under
/// the *coherent* part, so a change that quietly stops decoding is visible.
#[test]
fn the_census_walk_stays_coherent_over_most_of_its_instructions() {
    let Some(c) = run() else {
        eprintln!("[skip] extracted/PROT not present");
        return;
    };
    assert!(
        c.census.records > 5_000,
        "records walked: {}",
        c.census.records
    );
    let clean: usize = c.census.clean.values().sum();
    let total: usize = c.census.total.values().sum();
    assert!(clean > 0 && total >= clean);
    assert!(
        clean * 2 > total / 2,
        "clean {clean} of total {total} decoded instructions"
    );
}
