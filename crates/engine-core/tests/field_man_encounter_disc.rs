//! Disc-gated: the field scene-entry path installs the disc-resident
//! random-encounter table straight from a scene's MAN asset.
//!
//! This is the runtime sibling of `encounter_man_real.rs` (which decodes a
//! MAN in isolation): here we boot a real field scene through
//! `SceneHost::enter_field_scene` and assert the encounter session + the
//! merged formation defs come out of the scene's MAN - including the
//! `count=6` `scene_asset_table` field scenes (town01 / town0c) that the
//! relaxed detector now resolves.
//!
//! What this catches:
//! - `field_man_encounter_table` regresses to `None` for a MAN-backed scene.
//! - The MAN row-index `formation_id`s stop matching the merged
//!   `FormationDef`s (so a triggered encounter resolves to no monster set).
//! - The `count=6` detector regresses, dropping town01's MAN entirely.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn field_entry_installs_man_encounter_table() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    // town01 (Rim Elm) + town0c are `count=6` scene_asset_table scenes;
    // map03 is a kingdom-bundle (`count=7`) world-map scene. All carry a MAN
    // with a populated encounter section, so all must resolve a table - the
    // `count=6` cases only resolve thanks to the relaxed detector.
    for scene in ["town01", "town0c", "map03"] {
        host.enter_field_scene(scene, 0)
            .unwrap_or_else(|e| panic!("enter_field_scene('{scene}') failed: {e:#}"));

        // The MAN must resolve through the bundle (count=6 or count=7).
        let resolved = host.scene.as_ref().and_then(|s| {
            s.field_man_encounter_table(&host.index, scene)
                .ok()
                .flatten()
        });

        match resolved {
            Some((table, formations)) => {
                eprintln!(
                    "[{scene}] MAN encounter: {} entries, rate_q8={}, {} formation defs",
                    table.entries.len(),
                    table.trigger_rate_q8,
                    formations.len()
                );
                // The table installed at scene entry must be exactly this one.
                let session = host
                    .world
                    .encounter
                    .as_ref()
                    .expect("enter_field_scene installs the MAN encounter session");
                assert_eq!(
                    session.tracker().table().entries.len(),
                    table.entries.len(),
                    "[{scene}] installed session table size != resolved MAN table size"
                );
                // Every entry's row-index formation_id resolves to a merged def.
                for e in &table.entries {
                    assert!(
                        host.world
                            .formation_table
                            .formation(e.formation_id)
                            .is_some(),
                        "[{scene}] formation_id {} has no merged FormationDef",
                        e.formation_id
                    );
                }
            }
            None => {
                // All three scenes in this list carry a populated encounter
                // section, so `None` means the bundle/MAN resolution (or the
                // count=6 detector) regressed.
                panic!(
                    "[{scene}] expected a MAN encounter table to resolve via the \
                     scene bundle, got None (count=6 detector or MAN extract regressed?)"
                );
            }
        }
    }
}
