//! Disc-gated: the scene-prescript consumer census - the resolution of the
//! "dual consumer" open thread.
//!
//! The per-scene prescript bundle (`[count][offsets]` records) turns out to
//! have **one** consumer: the move-VM stager installer (`FUN_800252EC`).
//! Field-VM op `0x34` sub-`3` carries a literal record id, and the ops live
//! in the scene MAN's scripts - partition-1 dedicated effect-actor records
//! (Shift-JIS-named "effect", body = "install id N + loop") plus
//! interaction scripts, and partition-2 cutscene timelines (installing the
//! per-shot effect ids). Record 0 is a stager too - typically the scene's
//! master ambient record (the 8-byte-periodic spawn rows on towns),
//! installed on scene entry by the effect actor. No field-VM consumer of
//! prescript bytes exists; the field VM's scripts are the MAN's own
//! partitions.
//!
//! The id space is pinned live: retail relocates the bundle to RAM as a
//! compact `[u16 count][u16 offsets[count]]` table at `_DAT_8007B8D0`
//! (`FUN_800252EC` reads `base + 2 + id*2`), and the town01 field state
//! shows count = the file's record count with record 0's bytes at the first
//! offset - so installer id `k` = prescript record `k`, with no skip.
//!
//! Skips when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing; the
//! live-RAM pin additionally needs the save library.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_stager_installs;

fn extracted_prot() -> Option<PathBuf> {
    for p in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(p);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn load_man_from_scene(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = legaia_asset::scene_asset_table::detect(bytes)?;
    let man = table
        .descriptors
        .iter()
        .find(|d| d.type_byte == 0x03)
        .copied()?;
    let start = man.data_offset as usize;
    if start >= bytes.len() {
        return None;
    }
    let (decoded, _) = legaia_lzs::decompress_tracked(&bytes[start..], man.size as usize).ok()?;
    (decoded.len() == man.size as usize).then_some(decoded)
}

/// The prescript record ranges for a scene entry file, when it carries one
/// of the two container shapes.
fn prescript_records(bytes: &[u8]) -> Option<Vec<(usize, usize)>> {
    legaia_asset::scene_event_scripts::record_ranges(bytes)
        .or_else(|| legaia_asset::scene_scripted_asset_table::record_ranges(bytes))
        .filter(|r| !r.is_empty())
}

/// One scene block: CDNAME label, prescript records, decoded MAN bytes.
type SceneBlock = (String, Vec<Vec<u8>>, Vec<u8>);
/// A block mid-collection (either carrier may not have been seen yet).
type PartialBlock = (String, Option<Vec<Vec<u8>>>, Option<Vec<u8>>);

/// Walk `extracted/PROT` grouping consecutive same-label entries into scene
/// blocks, pairing each block's prescript carrier with its MAN carrier.
fn collect_scenes(prot: &PathBuf) -> Vec<SceneBlock> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(prot)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "BIN"))
        .collect();
    entries.sort();

    let mut current: Option<PartialBlock> = None;
    let mut finished: Vec<SceneBlock> = Vec::new();
    for path in &entries {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let label = stem
            .split_once('_')
            .map(|(_, l)| l)
            .unwrap_or("")
            .to_string();
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let slice_records = |ranges: Vec<(usize, usize)>| {
            ranges
                .iter()
                .map(|&(s, e)| bytes[s..e.min(bytes.len())].to_vec())
                .collect::<Vec<_>>()
        };
        match &mut current {
            Some((l, pre, man)) if *l == label => {
                if pre.is_none()
                    && let Some(ranges) = prescript_records(&bytes)
                {
                    *pre = Some(slice_records(ranges));
                }
                if man.is_none() {
                    *man = load_man_from_scene(&bytes);
                }
            }
            _ => {
                if let Some((l, Some(p), Some(m))) = current.take() {
                    finished.push((l, p, m));
                }
                let pre = prescript_records(&bytes).map(slice_records);
                current = Some((label, pre, load_man_from_scene(&bytes)));
            }
        }
    }
    if let Some((l, Some(p), Some(m))) = current.take() {
        finished.push((l, p, m));
    }
    finished
}

#[test]
fn prescript_stager_ids_census() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    let mut scenes = 0usize;
    let mut scenes_with_installs = 0usize;
    let mut p1_installs = 0usize;
    let mut p2_installs = 0usize;
    let mut in_range_ids = 0usize;
    let mut out_of_range: Vec<(String, u8, usize)> = Vec::new();
    let mut record0_scenes = 0usize;

    for (label, records, man) in &collect_scenes(&prot) {
        let Ok(mf) = legaia_asset::man_section::parse(man) else {
            continue;
        };
        scenes += 1;
        let installs = scene_stager_installs(&mf, man);
        if installs.is_empty() {
            continue;
        }
        scenes_with_installs += 1;
        let mut ids: BTreeSet<u8> = BTreeSet::new();
        for i in &installs {
            ids.insert(i.stager_id);
            match i.partition {
                2 => p2_installs += 1,
                _ => p1_installs += 1,
            }
        }
        for &id in &ids {
            if (id as usize) < records.len() {
                in_range_ids += 1;
            } else {
                out_of_range.push((label.clone(), id, records.len()));
            }
        }
        record0_scenes += usize::from(ids.contains(&0));
    }

    eprintln!(
        "[prescript] scenes={scenes} with_installs={scenes_with_installs} \
         p1_installs={p1_installs} p2_installs={p2_installs} in_range_ids={in_range_ids} \
         record0_scenes={record0_scenes} out_of_range={out_of_range:?}"
    );

    assert!(scenes > 50, "expected the full scene corpus, got {scenes}");
    // The stager installer is broadly used from both script homes.
    assert!(
        scenes_with_installs > 60,
        "expected most scenes to install stagers, got {scenes_with_installs}"
    );
    assert!(
        p1_installs > 50 && p2_installs > 50,
        "expected installs from both the placement scripts (p1={p1_installs}) and the \
         cutscene timelines (p2={p2_installs})"
    );
    // Referenced ids index the prescript record space directly (id k =
    // record k, live-pinned by the RAM table below) - essentially every
    // referenced id is a valid record. The linear over-walk yields a couple
    // of phantom decodes on message text; bound them rather than pretending
    // the walker is exact.
    assert!(
        in_range_ids > 250,
        "expected a large in-range id census, got {in_range_ids}"
    );
    assert!(
        out_of_range.len() <= 3,
        "too many out-of-range ids for the phantom budget: {out_of_range:?}"
    );
    // Record 0 is itself an installable stager (the master ambient record),
    // referenced by the entry effect-actor scripts in most scenes.
    assert!(
        record0_scenes > 50,
        "expected record 0 to be a widely installed stager, got {record0_scenes}"
    );
}

/// Live pin of the id space: retail relocates the prescript bundle to RAM
/// as `[u16 count][u16 offsets[count]]` at `_DAT_8007B8D0` (the table
/// `FUN_800252EC` indexes). In the town01 field state the count equals the
/// file bundle's record count and the first offset lands on record 0's
/// bytes - installer id `k` = prescript record `k`.
#[test]
fn stager_table_ram_relocation_matches_the_file_bundle() {
    use legaia_mednafen::{SaveState, ScenarioManifest};

    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing");
        return;
    };
    let manifest_path = ["scripts/scenarios.toml", "../scripts/scenarios.toml"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists());
    let library = ["saves/library", "../saves/library"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir());
    let (Some(manifest_path), Some(library)) = (manifest_path, library) else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let Some(scn) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "v0_1_pre_battle_tetsu")
    else {
        eprintln!("[skip] town01 field scenario missing from the manifest");
        return;
    };
    let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
        eprintln!("[skip] scenario has no library backup");
        return;
    };
    if !save_path.exists() {
        eprintln!("[skip] library backup not present");
        return;
    }

    // File side: town01's prescript bundle.
    let town01 = collect_scenes(&prot)
        .into_iter()
        .find(|(l, _, _)| l == "town01")
        .expect("town01 prescript + MAN");
    let records = town01.1;

    // RAM side: the relocated table.
    let state = SaveState::from_path(&save_path).expect("parse save state");
    let ram = state.main_ram().expect("main RAM");
    let va = |addr: u32| (addr & 0x001F_FFFF) as usize;
    let base = u32::from_le_bytes(
        ram[va(0x8007_B8D0)..va(0x8007_B8D0) + 4]
            .try_into()
            .unwrap(),
    );
    assert!(
        (0x8000_0000..0x8020_0000).contains(&base),
        "stager-table base pointer out of RAM: {base:#X}"
    );
    let tbl = va(base);
    let count = u16::from_le_bytes(ram[tbl..tbl + 2].try_into().unwrap()) as usize;
    assert_eq!(count, records.len(), "RAM table count vs file record count");
    // First offset -> record 0's bytes, byte-identical to the file record.
    let off0 = u16::from_le_bytes(ram[tbl + 2..tbl + 4].try_into().unwrap()) as usize;
    let r0 = &records[0];
    assert_eq!(
        &ram[tbl + off0..tbl + off0 + r0.len().min(64)],
        &r0[..r0.len().min(64)],
        "RAM record 0 vs file record 0"
    );
    eprintln!(
        "[prescript-ram] base={base:#X} count={count} off0={off0:#X} record0 matches ({} bytes checked)",
        r0.len().min(64)
    );
}
