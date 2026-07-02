//! Disc-gated: per-scene BGM ids recovered from the scene MANs, joined to
//! the curated music-track label table.
//!
//! The field-VM BGM start (op `0x35` sub `1`) carries its id as a literal
//! operand, so "which track does each scene's script start" is disc-sourced
//! data: [`scene_bgm_starts`] walks every scene MAN's partition-1 scripts
//! and decodes the ops. Global-pool ids (`>= 2000`) index the `music_01`
//! PROT bank, whose slot order is the debug sound-test order the curated
//! `legaia_gamedata` music table is keyed on - so every global id resolves
//! to a human-readable track label via [`music_labels`]. Skips when
//! `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::scene_bgm_starts;
use legaia_engine_core::music_labels;

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

#[test]
fn scene_bgm_ids_resolve_to_music_labels() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = extracted_prot() else {
        eprintln!("[skip] extracted/PROT/ missing - run `legaia-extract` first");
        return;
    };

    // entry-file-name -> sorted distinct bgm ids started by that scene's scripts
    let mut found: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    let mut scenes_with_man = 0usize;

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "BIN"))
        .collect();
    entries.sort();

    for path in entries {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(man) = load_man_from_scene(&bytes) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        scenes_with_man += 1;
        let starts = scene_bgm_starts(&mf, &man);
        if starts.is_empty() {
            continue;
        }
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let ids = found.entry(name).or_default();
        for s in starts {
            if !ids.contains(&s.bgm_id) {
                ids.push(s.bgm_id);
            }
        }
    }

    let mut global = 0usize;
    let mut local = 0usize;
    let mut unlabeled: Vec<(String, u16)> = Vec::new();
    for (scene, ids) in &found {
        let labels: Vec<String> = ids
            .iter()
            .map(|&id| match music_labels::label_for_bgm_id(id) {
                Some(l) => {
                    global += usize::from(id >= 2000);
                    local += usize::from(id < 2000);
                    format!("{id}={l}")
                }
                None => {
                    if id >= 2000 {
                        unlabeled.push((scene.clone(), id));
                    } else {
                        local += 1;
                    }
                    format!("{id}=?")
                }
            })
            .collect();
        eprintln!("[bgm] {scene}: {}", labels.join(" | "));
    }

    eprintln!(
        "[bgm] scenes_with_man={scenes_with_man} scenes_with_bgm={} global_ids={global} local_ids={local}",
        found.len()
    );

    assert!(
        scenes_with_man > 50,
        "expected the full scene corpus, got {scenes_with_man}"
    );
    assert!(
        found.len() > 20,
        "expected many scenes to start BGM, got {}",
        found.len()
    );
    // Every global-pool id must resolve to a curated label: the music_01
    // bank slot order is the sound-test order the table is keyed on.
    assert!(
        unlabeled.is_empty(),
        "global BGM ids without a music-table row: {unlabeled:?}"
    );

    // Semantic anchors - the join is only right if the labels match the
    // scenes that start them. Rim Elm's town scripts start the Rim Elm
    // theme; the three kingdom world maps start the overworld pair; Sol's
    // casino floor starts the casino theme; Jeremi starts its own theme.
    let anchor = |scene: &str, id: u16| {
        assert!(
            found.get(scene).is_some_and(|ids| ids.contains(&id)),
            "expected scene {scene} to start global BGM id {id} ({:?})",
            found.get(scene)
        );
    };
    anchor("0004_town01", 2016); // #16 M14B "Rim Elm theme"
    anchor("0086_map01", 2000); // #0 M01 "Overworld with mist"
    anchor("0086_map01", 2001); // #1 M02 "Overworld with no mist"
    anchor("0245_map02", 2000);
    anchor("0392_map03", 2000);
    anchor("0543_koin1", 2018); // #18 M16 "Sol casino"
    anchor("0166_geremi", 2047); // #47 M102 "Jeremi"
    anchor("0053_bylon", 2019); // #19 M17 "Byron Monastery"
}
