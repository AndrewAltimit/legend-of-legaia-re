//! Disc-gated: a scene's regions are **story-flag-gated groups**, not one flat
//! list, and a walking player fights the group the flag bank selects.
//!
//! The MAN encounter section's condition array partitions the region array into
//! consecutive groups (`FUN_801D9E1C` `0x801d9f30..0x801d9fd8`). The roll walks
//! the conditions in order, skipping each group whose story flag is clear, and
//! searches only the first group whose flag is set - or the `0xFFFF`
//! unconditional tail every retail scene ends with. Reading the array flat
//! instead lands on group 0, which in most scenes is a story variant whose
//! rows are whole-map `rate 0` placeholders; a first-match lookup stops there
//! and the scene goes silent.
//!
//! Three things are pinned here:
//!   1. *structure* - across every retail scene bundle the group lengths tile
//!      the region array exactly, and the condition list ends with exactly one
//!      unconditional record and contains no other. This is the whole basis of
//!      the group reading; if it were wrong, it would be wrong loudly.
//!   2. *player-denominated* - scenes whose real regions sit behind a leading
//!      gated group now produce an actual battle for a stepping player, and the
//!      formation it rolls is a real row of that scene's own formation table.
//!   3. *the honest negative* - scenes that carry a single unconditional group
//!      of rate-0 rows are encounter-free by data in every story state. They
//!      are not a defect and must not be counted as one.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::man_section;
use legaia_engine_core::encounter::EncounterPhase;
use legaia_engine_core::region_encounter::{DEFAULT_GROUP_FLAG, EncounterRegion};
use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

/// Field scenes whose live region set sits behind at least one flag-gated
/// group, so a flat read of the region array resolves to a dead row.
const GATED_SCENES: &[&str] = &["deroa", "chitei2", "dohaty", "retock", "geremi", "kor"];

/// Scenes whose entire condition list is one unconditional group of rate-0
/// rows: encounter-free in every story state, by data.
const SILENT_SCENES: &[&str] = &["keikoku", "suimon", "uru", "station3"];

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn disc_ready() -> Option<PathBuf> {
    let Some(d) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    Some(d)
}

fn rollable(r: &EncounterRegion) -> bool {
    r.rate_increment > 0 && r.formation_count > 0
}

#[test]
fn condition_groups_tile_the_region_array_in_every_scene() {
    let Some(extracted) = disc_ready() else {
        return;
    };
    let prot = extracted.join("PROT");
    if !prot.is_dir() {
        eprintln!("[skip] extracted/PROT missing");
        return;
    }

    let mut entries: Vec<PathBuf> = std::fs::read_dir(&prot)
        .expect("read extracted/PROT")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    entries.sort();

    let mut checked = 0usize;
    let mut gated = 0usize;
    for path in &entries {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Some(table) = legaia_asset::scene_asset_table::detect(&bytes) else {
            continue;
        };
        let Some(desc) = table.descriptors.iter().find(|d| d.type_byte == 0x03) else {
            continue;
        };
        let start = desc.data_offset as usize;
        if start >= bytes.len() {
            continue;
        }
        let Ok((man_bytes, _)) =
            legaia_lzs::decompress_tracked(&bytes[start..], desc.size as usize)
        else {
            continue;
        };
        let Ok(man) = man_section::parse(&man_bytes) else {
            continue;
        };
        let Some(body) = man.encounter_section_body(&man_bytes) else {
            continue;
        };
        let Ok(es) = man_section::parse_encounter_section(body) else {
            continue;
        };
        checked += 1;
        let label = path.file_name().unwrap().to_string_lossy();

        let conds: Vec<_> = man_section::condition_records(body, &es)
            .map(|c| c.unwrap_or_else(|| panic!("[{label}] condition record parses")))
            .collect();
        assert!(
            !conds.is_empty(),
            "[{label}] a scene with regions must carry conditions - retail's roll \
             exits on `walked == condition_count`, so zero conditions means zero \
             encounters, not 'use every region'"
        );
        let summed: usize = conds.iter().map(|c| c.region_count as usize).sum();
        assert_eq!(
            summed, es.region_count as usize,
            "[{label}] condition group lengths tile the region array exactly"
        );
        let defaults = conds.iter().filter(|c| c.is_default()).count();
        assert_eq!(
            defaults, 1,
            "[{label}] exactly one unconditional group per scene"
        );
        assert!(
            conds.last().expect("non-empty").is_default(),
            "[{label}] the unconditional group is the last condition - an earlier \
             one would make every group behind it unreachable"
        );
        if conds.len() > 1 {
            gated += 1;
        }
    }

    assert!(
        checked > 50,
        "the corpus walk must reach the retail scene bundles (reached {checked})"
    );
    eprintln!(
        "[groups] {checked} scene MANs walked; {gated} carry at least one \
         story-flag-gated region group"
    );
}

#[test]
fn a_walking_player_fights_in_scenes_whose_regions_are_flag_gated() {
    let Some(extracted) = disc_ready() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    let mut driven = 0usize;
    for scene in GATED_SCENES {
        if host.enter_field_scene(scene, 0).is_err() {
            eprintln!("[{scene}] not enterable as a field scene (skip)");
            continue;
        }
        let Some(tracker) = host.world.field_region_tracker.as_ref() else {
            eprintln!("[{scene}] no region tracker installed (skip)");
            continue;
        };
        let table = tracker.table();

        // The defect's shape: read flat, the very first region is a whole-map
        // row that rolls nothing, so a first-match lookup finds it and stops.
        let flat_first = table.regions.first().copied().expect("regions non-empty");
        assert!(
            !rollable(&flat_first),
            "[{scene}] fixture assumption: region 0 is a non-rolling row"
        );
        let group = table
            .active_group()
            .unwrap_or_else(|| panic!("[{scene}] the condition walk selects a group"));
        assert_eq!(
            group.flag_id, DEFAULT_GROUP_FLAG,
            "[{scene}] a cleared flag bank selects the unconditional tail"
        );
        assert!(
            group.start > 0,
            "[{scene}] the live group is not group 0 - that is the whole point"
        );
        assert!(
            table.any_rollable(),
            "[{scene}] the live group can produce a battle"
        );

        // Seat the player on a tile whose *resolved* region (first match inside
        // the live group, retail's rule) rolls.
        let mut seat = None;
        for r in table.active_regions() {
            if !rollable(r) {
                continue;
            }
            let cx = ((r.tile_x_min as i32 + r.tile_x_max as i32) / 2) * 128;
            let cz = ((r.tile_z_min as i32 + r.tile_z_max as i32) / 2) * 128;
            if let Some(res) = table.region_at_world(cx as i16, cz as i16)
                && rollable(res)
            {
                seat = Some((cx as i16, cz as i16));
                break;
            }
        }
        let Some((px, pz)) = seat else {
            eprintln!("[{scene}] every rollable region is shadowed (skip)");
            continue;
        };

        // The scene's own formation table - the id space the roll must land in.
        let man = host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&host.index).ok().flatten())
            .expect("field MAN payload");
        let man_file = man_section::parse(&man).expect("MAN parses");
        let body = man_file
            .encounter_section_body(&man)
            .expect("encounter section body");
        let es = man_section::parse_encounter_section(body).expect("encounter section parses");
        let formations: Vec<_> = man_section::formation_records(body, &es).collect();

        let slot = host
            .world
            .player_actor_slot
            .unwrap_or_else(|| panic!("[{scene}] field scene has a player actor"));
        let actor = host
            .world
            .actors
            .get_mut(slot as usize)
            .expect("player actor slot populated");
        actor.move_state.world_x = px;
        actor.move_state.world_z = pz;

        let mut fired = false;
        for _ in 0..20_000 {
            if host.world.on_field_step() {
                fired = true;
                break;
            }
        }
        assert!(
            fired,
            "[{scene}] a player standing in the live group's region meets something"
        );
        assert!(
            matches!(
                host.world.encounter.as_ref().map(|s| s.phase()),
                Some(EncounterPhase::Transition { .. })
            ),
            "[{scene}] the region roll drove the session's transition SM"
        );

        let mut roll = None;
        for _ in 0..1024 {
            host.world.tick_encounter();
            if let Some(r) = host.world.drain_encounter_formation() {
                roll = Some(r);
                break;
            }
        }
        let roll = roll.unwrap_or_else(|| panic!("[{scene}] transition resolves to a roll"));
        let row = formations
            .get(roll.formation_id as usize)
            .unwrap_or_else(|| {
                panic!(
                    "[{scene}] formation {} is inside the scene's own table ({} rows)",
                    roll.formation_id,
                    formations.len()
                )
            })
            .unwrap_or_else(|| panic!("[{scene}] formation {} parses", roll.formation_id));
        assert!(
            row.monster_count > 0,
            "[{scene}] formation {} spawns at least one monster",
            roll.formation_id
        );
        eprintln!(
            "[groups] '{scene}': live group {}..{} -> formation {} ({} monsters)",
            group.start,
            group.start + group.len,
            roll.formation_id,
            row.monster_count
        );
        driven += 1;
        host.world.end_encounter_battle();
    }

    assert!(
        driven >= 3,
        "at least three flag-gated scenes drive a real encounter (drove {driven})"
    );
}

#[test]
fn scenes_with_one_unconditional_rate_zero_group_are_silent_by_data() {
    let Some(extracted) = disc_ready() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

    let mut checked = 0usize;
    for scene in SILENT_SCENES {
        if host.enter_field_scene(scene, 0).is_err() {
            eprintln!("[{scene}] not enterable as a field scene (skip)");
            continue;
        }
        let Some(tracker) = host.world.field_region_tracker.as_ref() else {
            eprintln!("[{scene}] no region tracker installed (skip)");
            continue;
        };
        let table = tracker.table();
        assert_eq!(
            table.groups.len(),
            1,
            "[{scene}] one condition group - no story state adds another"
        );
        assert_eq!(table.groups[0].flag_id, DEFAULT_GROUP_FLAG);
        assert!(
            !table.regions.iter().any(rollable),
            "[{scene}] every region is rate-0 / empty-range: no flag can turn \
             this scene's encounters on, so a silent walk here is retail \
             behaviour and not a port defect"
        );
        assert!(!table.any_rollable(), "[{scene}] cached answer agrees");
        checked += 1;
    }
    assert!(
        checked >= 2,
        "at least two of the unconditionally-silent scenes were reachable (got {checked})"
    );
    eprintln!("[groups] {checked} scenes confirmed encounter-free by data");
}
