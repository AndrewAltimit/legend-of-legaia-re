//! Disc-gated: the **cast-effect pool** - PROT 0903..0966's DATA layer -
//! resolves for every player Seru cast and for a capture-class cast, and the
//! effect stand-in actually stages the module's records in a live battle.
//!
//! Three tiers, each answering a different question:
//!
//! 1. **Parity with the doc.** `docs/subsystems/cast-module.md` quotes the
//!    spawn-site / record counts of the three decoded exemplars (PROT 958 =
//!    "13 distinct records for 15 sites", 959 = 41 for 41, 960 = 21 for 24) and
//!    their record spans. Those are the figures the band's own bytes give, so a
//!    regression in `summon_overlay`'s `a2` recovery moves them.
//! 2. **Resolution.** Every id in the player Seru block `0x81..=0x8B` names a
//!    band entry that is loaded and carries records, and a capture-class id
//!    resolves through the *other* dispatcher's key (the spell record's `+0x01`
//!    byte) onto the module the doc names for it.
//! 3. **Output under a real session.** A pad-driven battle casts each player
//!    Seru spell and the module's records are staged as an effect scene - the
//!    thing that used to be the cast band's open half.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool, capture_module_prot,
    seru_module_prot,
};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use std::path::PathBuf;
use std::sync::Arc;

/// Player Seru-magic ids the magic menu can cast.
const SERU_IDS: [u8; 11] = [
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b,
];

/// `docs/subsystems/cast-module.md`: the three decoded exemplars, as
/// `(PROT entry, spawn sites, distinct records, first record VA, last record VA)`.
const EXEMPLARS: [(u32, usize, usize, u32, u32); 3] = [
    (958, 15, 13, 0x801F_8EB8, 0x801F_9348),
    (959, 41, 41, 0x801F_884C, 0x801F_95CC),
    (960, 24, 21, 0x801F_8768, 0x801F_8E0C),
];

/// The band's two record-less entries: PROT 0926 is the 1-sector null stub
/// (2040 of its 2048 bytes are PROT 0925's residue) and PROT 0952's two spawn
/// sites sit in its inherited tail (file `+0x11E8..+0x1800`, PROT 0951's
/// bytes), resolving to `0x801F8348` / `0x801F836C` - two of PROT 0951's
/// records, past the end of 0952's `0x1800`-byte image.
const RECORDLESS: [u32; 2] = [926, 952];

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() && p.join("SCUS_942.54").is_file() {
            return Some(p);
        }
    }
    None
}

/// Build the pool the scene host builds, straight off `PROT.DAT`.
fn build_pool(dir: &std::path::Path) -> CastEffectPool {
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let mut pool = CastEffectPool::new();
    for idx in CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST {
        let entry = archive
            .entries
            .get(idx as usize)
            .cloned()
            .unwrap_or_else(|| panic!("PROT {idx} entry"));
        let mut bytes = Vec::new();
        archive
            .read_entry(&entry, &mut bytes)
            .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
        assert!(pool.insert(idx, &bytes), "PROT {idx} is in the band");
    }
    pool
}

#[test]
fn the_band_parses_to_the_record_counts_the_doc_quotes() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] cast-effect pool parity: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let pool = build_pool(&dir);
    println!("[ok] cast-effect pool: {} band entries loaded", pool.len());
    assert_eq!(pool.len(), 64, "the whole 0903..0966 band");

    for (entry, sites, records, first_va, last_va) in EXEMPLARS {
        let m = pool.module(entry).expect("exemplar loaded");
        assert_eq!(m.spawn_sites, sites, "PROT {entry} spawn sites");
        assert_eq!(m.parts.len(), records, "PROT {entry} distinct records");
        let base = legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        assert_eq!(
            base + m.parts.first().unwrap().record_off as u32,
            first_va,
            "PROT {entry} first record VA"
        );
        assert_eq!(
            base + m.parts.last().unwrap().record_off as u32,
            last_va,
            "PROT {entry} last record VA"
        );
        println!("[ok] PROT {entry}: {sites} sites -> {records} records");
    }

    // Every other entry stages at least one record; the two that do not are
    // the documented pair, and a regression that emptied a third would show.
    let empty: Vec<u32> = pool
        .entries()
        .filter(|m| m.is_empty())
        .map(|m| m.prot_entry)
        .collect();
    assert_eq!(empty, RECORDLESS, "only the documented record-less entries");
}

#[test]
fn every_player_seru_cast_and_a_capture_cast_resolve_a_record_set() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] cast module resolution: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let pool = build_pool(&dir);
    let scus = std::fs::read(dir.join("SCUS_942.54")).expect("SCUS_942.54");

    // The action-id dispatcher (`FUN_801F1ED4`): row `id - 0x81`.
    for id in SERU_IDS {
        let entry = seru_module_prot(id).expect("player Seru ids are in the table span");
        assert_eq!(entry, 903 + u32::from(id - 0x81));
        let m = pool.module(entry).expect("module loaded");
        assert!(
            !m.parts.is_empty(),
            "spell {id:#04x} -> PROT {entry} stages records"
        );
    }

    // The class dispatcher (`FUN_801F2160`): row = the record's `+0x01` byte.
    // Every capture-class record on the disc must land inside the band, and
    // Blazing Slash `0x79` must land on the exemplar the doc decodes.
    let records =
        legaia_asset::spell_names::capture_class_records(&scus).expect("SCUS is a PSX-EXE");
    assert!(
        !records.is_empty(),
        "the disc carries capture-class records"
    );
    for (spell, sub) in &records {
        let entry = capture_module_prot(*sub)
            .unwrap_or_else(|| panic!("capture spell {spell:#04x} sub {sub} is out of the table"));
        assert!(
            pool.module(entry).is_some(),
            "capture spell {spell:#04x} -> PROT {entry} is a band entry"
        );
    }
    assert_eq!(
        records.iter().find(|(id, _)| *id == 0x79).map(|(_, s)| *s),
        Some(23),
        "Blazing Slash's +0x01 sub-id"
    );

    // ... and the World resolves the same way, through the disc spell table.
    let mut w = World::default();
    w.enter_battle(3, 1);
    w.install_menu_text(&scus);
    w.install_cast_effect_pool(Arc::new(pool));
    assert_eq!(
        w.cast_module_for(0x79),
        Some(958),
        "a capture-class cast routes on +0x01, not on the action id"
    );
    assert_eq!(
        w.cast_module_for(0x81),
        Some(903),
        "a non-capture cast routes on the action id"
    );
    assert!(
        w.spawn_cast_module_fx(0x79, [0, 0, 0]),
        "the capture module's records stage"
    );
    assert_eq!(
        w.active_summon.as_ref().map(|s| s.parts.len()),
        Some(13),
        "PROT 958's 13 records are the staged scene"
    );
    println!(
        "[ok] cast module resolution: {} capture records",
        records.len()
    );
}

// ---------------------------------------------------------------------------
// Tier 3: the live session
// ---------------------------------------------------------------------------

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("cast_effect_pool_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w.battle_no_escape = true;
    w
}

fn enter_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

fn press(w: &mut World, b: PadButton) {
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    w.set_pad(0);
    let _ = w.tick();
}

fn wait_for_prompt(w: &mut World) -> bool {
    for _ in 0..0x400 {
        let _ = w.take_pending_summon_spawn();
        if w.battle_command.is_some() {
            return true;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    false
}

/// Top the party and the monster back up so eleven casts in one fight stay
/// deterministic (a dead caster or a dead target ends the run early).
///
/// Every HP write goes through `set_hp_synced`: a bare `hp` write leaves the
/// `hp != hp_display` pair with a zero accumulator, which is the absorbing
/// state the action SM's `0x51` bar-drain gate parks on forever.
fn refill(w: &mut World) {
    for i in 0..3 {
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.set_hp_synced(100);
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.mp = 250;
    }
    let ms = w.party_count as usize;
    w.actors[ms].battle.max_hp = 9000;
    w.actors[ms].battle.set_hp_synced(9000);
}

#[test]
fn a_live_cast_stages_its_module_records() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] live cast staging: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let pool = Arc::new(build_pool(&dir));

    let mut w = build_world();
    w.spell_catalog = legaia_engine_core::retail_magic::retail_seru_magic_catalog();
    // Teach the caster the whole player block at level 1.
    {
        let rec = &mut w.roster.members[0];
        let mut list = rec.spell_list();
        list.count = SERU_IDS.len() as u8;
        for (i, id) in SERU_IDS.iter().enumerate() {
            list.ids[i] = *id;
            list.levels[i] = 1;
        }
        rec.set_spell_list(list);
    }
    enter_battle(&mut w);
    w.install_cast_effect_pool(pool.clone());
    refill(&mut w);
    w.battle_magic[0] = 80;

    let mut cast = 0usize;
    let mut staged: Vec<(u8, usize)> = Vec::new();
    for _ in 0..80 {
        if staged.len() == SERU_IDS.len() {
            break;
        }
        refill(&mut w);
        if !wait_for_prompt(&mut w) {
            panic!(
                "command session never reopened after {} casts; mode={:?} state={:02X} \
                 active={} spell_menu={} arts={} item={} monster_hp={} party_hp={:?}",
                staged.len(),
                w.mode,
                w.battle_ctx.action_state,
                w.battle_ctx.active_actor,
                w.battle_spell_menu.is_some(),
                w.battle_arts_menu.is_some(),
                w.battle_item_menu.is_some(),
                w.actors[w.party_count as usize].battle.hp,
                (0..3).map(|i| w.actors[i].battle.hp).collect::<Vec<_>>(),
            );
        }
        if matches!(
            w.battle_command.as_ref().map(|s| &s.phase),
            Some(legaia_engine_core::battle_input::CommandPhase::RoundPrompt { .. })
        ) {
            press(&mut w, PadButton::Cross);
        }
        if w.battle_ctx.active_actor != 0 {
            press(&mut w, PadButton::Down); // Spirit - consumes the turn
            continue;
        }
        let row = staged.len();
        press(&mut w, PadButton::Right); // ring: Magic
        assert!(
            w.battle_spell_menu.is_some(),
            "the Magic arm opens the list"
        );
        // The list cursor is per-session, so walk from the top each time.
        for _ in 0..row {
            press(&mut w, PadButton::Down); // walk to this cast's row
        }
        let picked = w
            .battle_spell_menu
            .as_ref()
            .and_then(|s| s.menu_spell())
            .map(|r| r.id)
            .unwrap_or_else(|| panic!("no spell under the cursor at row {row}"));
        press(&mut w, PadButton::Cross); // pick the spell
        // A single-target shape opens the picker; a group shape commits
        // straight away, so only confirm when there is something to confirm.
        if w.battle_spell_menu
            .as_ref()
            .and_then(|s| s.picker())
            .is_some()
        {
            press(&mut w, PadButton::Cross); // confirm the target
        }
        cast += 1;
        for _ in 0..3 {
            if w.battle_command.is_none() {
                break;
            }
            press(&mut w, PadButton::Down);
        }
        // Run the band out, watching for the frame the stager stages the
        // module's records (`SummonPhase::Armed`, retail's `0x801E4B1C`).
        let mut seen: Option<usize> = None;
        for _ in 0..0x400 {
            if seen.is_none()
                && let Some(scene) = w.active_summon.as_ref()
            {
                seen = Some(scene.parts.len());
            }
            if w.pending_cast.is_none() {
                break;
            }
            let _ = w.take_pending_summon_spawn();
            w.set_pad(0);
            let _ = w.tick();
        }
        assert!(w.pending_cast.is_none(), "cast {cast} never folded");
        // The id the menu actually committed, not the row we aimed at.
        let id = picked;
        assert_eq!(id, SERU_IDS[row], "row {row} is this cast's spell");
        let parts = seen
            .unwrap_or_else(|| panic!("cast {cast} (spell {id:#04x}) staged no cast-module scene"));
        staged.push((id, parts));
        // Clear the scene so the next cast's staging is its own.
        w.active_summon = None;
    }

    assert_eq!(
        staged.len(),
        SERU_IDS.len(),
        "every player Seru spell cast and staged"
    );
    for (id, parts) in &staged {
        let entry = seru_module_prot(*id).unwrap();
        let expect = pool.module(entry).unwrap().parts.len();
        assert_eq!(
            *parts, expect,
            "spell {id:#04x} staged PROT {entry}'s whole record set"
        );
        assert!(*parts > 0);
        println!("[ok] spell {id:#04x} -> PROT {entry}: {parts} records staged");
    }
}
