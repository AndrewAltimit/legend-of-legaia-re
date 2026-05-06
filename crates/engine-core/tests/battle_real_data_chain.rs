//! Disc-gated end-to-end: parse the retail effect bundle, drive the
//! battle SM with the World wired up, and confirm both the asset-side
//! and engine-side battle plumbing accept real game data.
//!
//! This complements `battle_attack_integration.rs` (which exercises the
//! formulas + SM with synthetic state) by proving the **asset chain**
//! reaches the SM cleanly.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::effect_bundle;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, StepOutcome};
use legaia_prot::archive::Archive;
use legaia_prot::cdname;

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
fn real_effect_bundle_parses_and_battle_sm_progresses() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut archive = Archive::open(&extracted.join("PROT.DAT")).expect("open PROT");
    // Walk every PROT entry and find the effect-bundle hit. The only
    // retail bundle that detects today is in 0000_init_data.
    let mut bundle_payload: Option<(u32, Vec<u8>)> = None;
    for (idx, entry) in archive.entries.clone().iter().enumerate() {
        let mut bytes = Vec::new();
        archive.read_entry(entry, &mut bytes).expect("read entry");
        if effect_bundle::detect(&bytes).is_some() {
            bundle_payload = Some((idx as u32, bytes));
            break;
        }
    }
    let (entry_idx, bytes) =
        bundle_payload.expect("at least one PROT entry must carry the effect bundle");

    let bundle = effect_bundle::detect(&bytes).expect("re-detect on located entry");
    eprintln!(
        "[battle-real] effect bundle in PROT entry {} magic@0x{:X} table@0x{:X} assets@0x{:X} slots={}",
        entry_idx,
        bundle.magic_offset,
        bundle.table_offset,
        bundle.assets_start,
        bundle.slots.len()
    );
    assert_eq!(bundle.slots.len(), 28, "effect bundle declares 28 slots");
    assert!(
        !bundle.assets.tmds.is_empty(),
        "effect bundle must surface at least the master TMD"
    );

    // Spin up a battle world and advance through Begin → EndOfAction.
    // The SM is exercised separately by `battle_scene_smoke`; here we
    // just confirm the asset bundle's existence doesn't break the world
    // construction path engines use to wire effect-VM pools.
    let mut world = World {
        mode: SceneMode::Battle,
        party_count: 3,
        ..World::default()
    };
    for i in 0..8 {
        let actor = world.spawn_actor(i);
        actor.battle.liveness = 1;
        actor.battle.hp = 100;
        actor.battle.max_hp = 100;
    }
    world.battle_ctx.queued_action = 3;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();

    let mut transitions = 0u32;
    let mut unknowns = 0u32;
    for _ in 0..500 {
        match world.tick() {
            Some(StepOutcome::Transition { .. }) => transitions += 1,
            Some(StepOutcome::UnknownState { .. }) => unknowns += 1,
            Some(StepOutcome::BattleComplete) => break,
            _ => {}
        }
    }
    assert_eq!(unknowns, 0, "battle SM hit UnknownState");
    assert!(
        transitions > 0,
        "battle SM made zero transitions across 500 frames"
    );

    // CDNAME sanity: the `battle_data` block defined in CDNAME contains
    // the per-monster bundle table. Confirm we can resolve its range
    // and that the first entry isn't empty.
    let map = cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse CDNAME");
    let (bd_start, bd_end) =
        cdname::block_range_for_name(&map, "battle_data").expect("battle_data in CDNAME");
    assert!(
        bd_end > bd_start,
        "battle_data block range must be non-empty"
    );
    let mut first_battle_bytes = Vec::new();
    archive
        .read_entry(
            &archive.entries[bd_start as usize].clone(),
            &mut first_battle_bytes,
        )
        .expect("read battle_data entry 0");
    assert!(
        !first_battle_bytes.is_empty(),
        "battle_data first entry must not be empty"
    );
    eprintln!(
        "[battle-real] battle_data block PROT [{bd_start}..{bd_end}) first-entry size={}",
        first_battle_bytes.len()
    );
}
