//! Disc-gated: a **cold scene boot** (entering a scene directly with no New
//! Game confirm and no save loaded) seeds the retail new-game defaults - the
//! `SCUS_942.54` starting-party template ([`legaia_asset::new_game`]) and the
//! starting bag - so the pause menu always reads valid party data, and a
//! subsequent `save_full` / `load_full` round-trips that seeded state.
//!
//! Also pins the guard from the other side: a host with no defaults installed
//! (the disc-free construction path) keeps the empty scaffold roster, and a
//! world that loaded a save first is never re-seeded.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_core::Vfs;
use legaia_engine_core::new_game::NewGameDefaults;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::NEW_GAME_STARTING_GOLD;

fn disc_path() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    if !p.exists() {
        eprintln!("[skip] LEGAIA_DISC_BIN points at a missing file");
        return None;
    }
    Some(p)
}

/// Read + parse the new-game defaults from the disc's own executable - the
/// same source `BootSession::open_disc` wires into the host.
fn defaults_from_disc(disc: &std::path::Path) -> Option<NewGameDefaults> {
    let scus = legaia_engine_core::DiscVfs::open(disc)
        .ok()?
        .read("SCUS_942.54")
        .ok()?;
    let party = legaia_asset::new_game::StartingParty::from_scus(&scus)?;
    let inventory = legaia_asset::new_game::StartingInventory::from_scus(&scus);
    Some(NewGameDefaults { party, inventory })
}

#[test]
fn cold_scene_boot_seeds_new_game_defaults_and_round_trips() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let defaults = defaults_from_disc(&disc).expect("SCUS new-game template parses");
    let template_vahn = defaults.party.member(0).expect("template slot 0").clone();
    let seed_items: Vec<(u8, u8)> = defaults
        .inventory
        .as_ref()
        .map(|inv| inv.items().to_vec())
        .unwrap_or_default();
    assert!(
        !seed_items.is_empty(),
        "retail SCUS carries a starting-inventory seed"
    );

    // --- Baseline: without defaults installed, a cold boot keeps the empty
    // scaffold roster (the pre-existing behaviour disc-free tests rely on).
    let mut bare = SceneHost::open_disc(&disc).expect("open disc host");
    bare.enter_field_scene("town01", 0)
        .expect("enter town01 (bare)");
    assert!(
        bare.world.roster.members.is_empty(),
        "no defaults installed -> no seed"
    );

    // --- Cold boot with defaults installed (what BootSession / the browser
    // runtime wire): the template party + starting bag + gold appear.
    let mut host = SceneHost::open_disc(&disc).expect("open disc host");
    host.new_game_defaults = Some(defaults.clone());
    host.enter_field_scene("town01", 0).expect("enter town01");
    let w = &mut host.world;
    assert_eq!(w.party_count, 1, "retail New Game roster is Vahn alone");
    assert_eq!(w.roster.members.len(), 1);
    let rec = &w.roster.members[0];
    let hms = rec.hp_mp_sp();
    assert_eq!(hms.hp_max, template_vahn.hp_max, "template HP seeds the record");
    assert_eq!(hms.hp_cur, template_vahn.hp_max);
    assert_eq!(hms.mp_max, template_vahn.mp_max);
    let ls = rec.live_stats();
    assert_eq!(ls.agl, template_vahn.agl);
    assert_eq!(ls.atk, template_vahn.atk);
    assert_eq!(ls.spd, template_vahn.spd);
    assert_eq!(w.party_name(0), template_vahn.name, "template name seeds");
    assert_eq!(w.money, NEW_GAME_STARTING_GOLD);
    for &(id, count) in &seed_items {
        if id == 0 || count == 0 {
            continue;
        }
        assert_eq!(
            w.inventory.get(&id).copied(),
            Some(count),
            "starting-bag item {id:#04x} seeds with count {count}"
        );
    }

    // --- The seeded cold-boot state survives a full save/load cycle.
    let bytes = w.save_full().write();
    let parsed = legaia_save::SaveFile::parse(&bytes).expect("LGSF parses");
    let mut fresh = legaia_engine_core::world::World::new();
    fresh.load_full(parsed);
    assert_eq!(fresh.roster.members.len(), 1);
    assert_eq!(fresh.roster.members[0].hp_mp_sp().hp_max, template_vahn.hp_max);
    assert_eq!(fresh.money, NEW_GAME_STARTING_GOLD);
    for &(id, count) in &seed_items {
        if id == 0 || count == 0 {
            continue;
        }
        assert_eq!(fresh.inventory.get(&id).copied(), Some(count));
    }

    // --- And the guard: re-entering a scene (a door transition) never
    // re-seeds over live state.
    host.world.money = 4321;
    host.enter_field_scene("town01", 0).expect("re-enter town01");
    assert_eq!(host.world.money, 4321, "re-entry must not reset gold");
}
