//! Disc + save-library gated: the scene-entry spawn-prologue pre-run
//! (`World::pre_run_field_channel_prologues`, retail `FUN_8003A1E4`) seats
//! town01's placed actors where the retail engine seats them.
//!
//! Retail ground truth: the `v0_1_pre_battle_tetsu` mednafen capture (town01
//! free-roam, game_mode 0x03). Its field-actor list (`_DAT_8007C354` class,
//! per-node X `+0x14` / Z `+0x18` / script id `+0x50`) carries each
//! partition-1 placement's LIVE position - and a large share of them do NOT
//! stand at their MAN placement-header tile: the spawn-install prologue
//! (story-flag-tested `0x23 MoveTo` ops) parks despawned actors at the
//! off-map sentinel tile `(0x7F,0x7F)` and relocates others across the
//! scene. The engine's entry pre-run must reproduce that arrangement from
//! the same story-flag bank (`DAT_80085758`, seeded byte-for-byte from the
//! capture).
//!
//! Comparison contract per placement slot:
//! - retail parked (far-corner sentinel region) <-> engine parked;
//! - both placed: within the patrol-locality bound (retail walkers roam
//!   around their seat between capture and comparison; the seat itself is
//!   what the pre-run pins).
//!
//! Skips (passes) when `LEGAIA_DISC_BIN`, `extracted/`, the scenario
//! manifest, or the library backup is missing - CI runs without disc data.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};
use legaia_mednafen::{SaveState, ScenarioManifest};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Both coordinates in the far-corner park region (`tile 0x7F` = world
/// `16256..=16320`): the actor is off-field, not a visible placement.
fn is_parked(x: i16, z: i16) -> bool {
    x >= 0x3F00 && z >= 0x3F00
}

const RAM_MASK: u32 = 0x001F_FFFF;

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes(ram[o..o + 4].try_into().unwrap())
}

fn i16_at(ram: &[u8], va: u32) -> i16 {
    let o = (va & RAM_MASK) as usize;
    i16::from_le_bytes(ram[o..o + 2].try_into().unwrap())
}

#[test]
fn town01_entry_positions_match_retail_actor_list() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let manifest_path = [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists());
    let library = ["saves/library", "../saves/library", "../../saves/library"]
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

    // ---- Retail side: the field-actor list out of the capture's RAM. ----
    let state = SaveState::from_path(&save_path).expect("parse save state");
    let ram = state.main_ram().expect("main RAM");
    // Field-actor list (the `FUN_8003BC08` tick class): sentinel head at
    // `0x8007C34C + 2*4`, nodes chained through `+0x00`.
    let head = u32_at(ram, 0x8007_C354);
    let mut retail: Vec<(u8, i16, i16)> = Vec::new();
    let mut node = u32_at(ram, head);
    let mut hops = 0;
    while node != 0 && node != head && hops < 128 {
        let x = i16_at(ram, node + 0x14);
        let z = i16_at(ram, node + 0x18);
        let id50 = ram[((node + 0x50) & RAM_MASK) as usize];
        retail.push((id50, x, z));
        node = u32_at(ram, node);
        hops += 1;
    }
    assert!(
        retail.len() > 40,
        "expected the town01 actor census, got {} nodes",
        retail.len()
    );

    // ---- Engine side: seed the capture's story-flag bank, enter town01. ----
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    // `DAT_80085758` system-flag bank, byte-for-byte (engine layout matches:
    // byte `idx>>3`, bit `0x80 >> (idx&7)`).
    let bank_off = (0x8008_5758 & RAM_MASK) as usize;
    host.world.system_flags = ram[bank_off..bank_off + 0x1100].to_vec();
    host.enter_field_scene("town01", 0)
        .expect("enter town01 field scene");

    let scene = host.scene.as_ref().expect("scene loaded");
    let placements = scene
        .field_actor_placements(&host.index)
        .expect("placement decode")
        .expect("town01 has a MAN");
    let n0 = {
        let man = scene
            .field_man_payload(&host.index)
            .expect("man payload")
            .expect("man bytes");
        let mf = legaia_asset::man_section::parse(&man).expect("man parse");
        mf.header.partition_counts[0].max(0) as u8
    };

    // Patrol-locality bound for placed-vs-placed: the retail walker roams its
    // authored local route (<= 6 tiles) around the seat the pre-run pins.
    let locality = legaia_engine_core::man_field_scripts::NPC_ROUTE_LOCALITY;

    let mut compared = 0;
    let mut parked_both = 0;
    let mut failures: Vec<String> = Vec::new();
    for &(id50, rx, rz) in &retail {
        // Partition-1 placements only (`+0x50 = N0 + placement_index`);
        // partition-0 object actors and the special ids live elsewhere.
        if id50 < n0 {
            continue;
        }
        let pi = id50 - n0;
        let Some(p) = placements.iter().find(|p| p.index == pi as usize) else {
            continue;
        };
        let (ex, ez) = host
            .world
            .field_npc_positions
            .get(&pi)
            .copied()
            .unwrap_or((p.world_x, p.world_z));
        compared += 1;
        let (rp, ep) = (is_parked(rx, rz), is_parked(ex, ez));
        if rp && ep {
            parked_both += 1;
            continue;
        }
        if rp != ep {
            failures.push(format!(
                "placement {pi}: retail {} at ({rx},{rz}), engine {} at ({ex},{ez})",
                if rp { "PARKED" } else { "placed" },
                if ep { "PARKED" } else { "placed" },
            ));
            continue;
        }
        let (dx, dz) = ((ex as i32 - rx as i32).abs(), (ez as i32 - rz as i32).abs());
        if dx.max(dz) > locality {
            failures.push(format!(
                "placement {pi}: engine seat ({ex},{ez}) is {dx}/{dz} units from retail ({rx},{rz})"
            ));
        }
    }

    eprintln!(
        "[town01] compared {compared} placement actors ({parked_both} parked in both); \
         {} mismatch(es)",
        failures.len()
    );
    for f in &failures {
        eprintln!("  MISMATCH {f}");
    }
    // Non-vacuity: the capture really carries both classes.
    assert!(compared >= 30, "expected >=30 comparable placements");
    assert!(parked_both >= 10, "expected a parked cohort in both");
    assert!(
        failures.is_empty(),
        "{} placement seat(s) diverge from the retail actor list",
        failures.len()
    );
}
