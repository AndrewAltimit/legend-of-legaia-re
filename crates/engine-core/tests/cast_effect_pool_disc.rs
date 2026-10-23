//! Disc-gated: the **cast-effect pool** - PROT 0903..0966's DATA layer -
//! resolves for every player Seru cast and for a capture-class cast, and the
//! effect stand-in actually stages the module's records in a live battle.
//!
//! Three tiers, each answering a different question:
//!
//! 1. **Parity with the doc.** `docs/subsystems/cast-module.md` quotes the
//!    spawn-site / record counts of the three decoded exemplars (PROT 958 =
//!    "14 distinct records for 15 sites", 959 = 44 for 41, 960 = 23 for 24) and
//!    their record spans. Those are the figures the band's own bytes give, so a
//!    regression in `summon_overlay`'s `a2` recovery moves them.
//! 2. **Resolution.** Every id in the player Seru block `0x81..=0x8B` names a
//!    band entry that is loaded and carries records, and a capture-class id
//!    resolves through the *other* dispatcher's key (the spell record's `+0x01`
//!    byte) onto the module the doc names for it.
//! 3. **Output under a real session.** A pad-driven battle casts each player
//!    Seru spell and the module's records are staged as an effect scene - the
//!    thing that used to be the cast band's open half. A module seated whole
//!    stages its recovered record set on the stager's first tick; PROT 0903,
//!    whose director reports its spawn calls, seats them on the arms that
//!    make them (3 / 6 / 8), as its own disassembly does.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_cast::cast_module_camera as cmc;
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::{InputState, PadButton};
use legaia_overlay_images::cast_effect_pool::{
    CAST_MODULE_PROT_FIRST, CAST_MODULE_PROT_LAST, CastEffectPool, capture_module_prot,
    seru_module_prot,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Player Seru-magic ids the magic menu can cast.
const SERU_IDS: [u8; 11] = [
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b,
];

/// `docs/subsystems/cast-module.md`: the three decoded exemplars, as
/// `(PROT entry, spawn sites, distinct records, first record VA, last record VA)`.
const EXEMPLARS: [(u32, usize, usize, u32, u32); 3] = [
    (958, 15, 14, 0x801F_8EB8, 0x801F_9348),
    (959, 41, 44, 0x801F_884C, 0x801F_95CC),
    (960, 24, 23, 0x801F_8768, 0x801F_8E0C),
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
        let base = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
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
        legaia_game_tables::spell_names::capture_class_records(&scus).expect("SCUS is a PSX-EXE");
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
        w.casting.active_summon.as_ref().map(|s| s.parts.len()),
        Some(14),
        "PROT 958's 14 records are the staged scene"
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
    w.party.party_count = 3;
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
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_battle::encounter::{
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
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w.battle.no_escape = true;
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

/// One frame the way a headless driver runs it (`BootSession::tick`): the
/// scene tick, then the world's frame tail, whose effect scene-graph step
/// runs the cast module's parts - PROT 0909's two phase rendezvous are their
/// op-`0x20` calls.
fn frame(w: &mut World) {
    let _ = w.tick();
    let _ = w.step_world_frame_tail(None, None, |_| None);
}

fn press(w: &mut World, b: PadButton) {
    // The menu SM reads no pad while the prompt's glide is in flight
    // (`World::battle_command_pad_held`): wait it out, as a player does.
    for _ in 0..64 {
        if !w.battle_command_pad_held() {
            break;
        }
        w.set_pad(0);
        frame(w);
    }
    w.set_pad(InputState::mask_of([b]));
    frame(w);
    w.set_pad(0);
    let _ = w.tick();
}

fn wait_for_prompt(w: &mut World) -> bool {
    for _ in 0..0x400 {
        let _ = w.take_pending_summon_spawn();
        if w.battle.command.is_some() {
            return true;
        }
        w.set_pad(0);
        frame(w);
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
    let ms = w.party.party_count as usize;
    w.actors[ms].battle.max_hp = 9000;
    w.actors[ms].battle.set_hp_synced(9000);
}

/// Frames one cast's band is given to fold. The module directors hold each
/// arm as long as retail does: PROT 0906 (Gizam) runs past a thousand.
const BAND_FRAMES: usize = 0x1000;

#[test]
fn a_live_cast_stages_its_module_records() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] live cast staging: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let pool = Arc::new(build_pool(&dir));

    let mut w = build_world();
    w.tables.spell_catalog = legaia_engine_battle::retail_magic::retail_seru_magic_catalog();
    // Teach the caster the whole player block at level 1.
    {
        let rec = &mut w.party.roster.members[0];
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
    // The battle overlay the scene host retains at battle load: PROT 0903's
    // arm-8 breath is a PROT 0898 effect prototype (`*(0x801F63A8)`).
    {
        let mut archive =
            legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
        let entry = archive.entries[898].clone();
        let mut b898 = Vec::new();
        archive
            .read_entry(&entry, &mut b898)
            .expect("read PROT 0898");
        w.tables.move_power_overlay = Some(Arc::from(b898.as_slice()));
    }
    refill(&mut w);
    w.battle.magic[0] = 80;

    let mut cast = 0usize;
    // (spell id, parts on the first staged frame, parts seated over the band)
    let mut staged: Vec<(u8, usize, usize, std::collections::BTreeSet<u32>)> = Vec::new();
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
                w.battle.spell_menu.is_some(),
                w.battle.arts_menu.is_some(),
                w.battle.item_menu.is_some(),
                w.actors[w.party.party_count as usize].battle.hp,
                (0..3).map(|i| w.actors[i].battle.hp).collect::<Vec<_>>(),
            );
        }
        if matches!(
            w.battle.command.as_ref().map(|s| &s.phase),
            Some(legaia_engine_battle_menus::battle_input::CommandPhase::RoundPrompt { .. })
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
            w.battle.spell_menu.is_some(),
            "the Magic arm opens the list"
        );
        // The list cursor is per-session, so walk from the top each time.
        for _ in 0..row {
            press(&mut w, PadButton::Down); // walk to this cast's row
        }
        let picked = w
            .battle
            .spell_menu
            .as_ref()
            .and_then(|s| s.menu_spell())
            .map(|r| r.id)
            .unwrap_or_else(|| panic!("no spell under the cursor at row {row}"));
        press(&mut w, PadButton::Cross); // pick the spell
        // A single-target shape opens the picker; a group shape commits
        // straight away, so only confirm when there is something to confirm.
        if w.battle
            .spell_menu
            .as_ref()
            .and_then(|s| s.picker())
            .is_some()
        {
            press(&mut w, PadButton::Cross); // confirm the target
        }
        cast += 1;
        // The last commit raises the party's Begin | Reselect (`0x6E`);
        // Cross takes its highlighted Begin.
        for _ in 0..4 {
            match w.battle.command.as_ref().map(|c| &c.phase) {
                None => break,
                Some(legaia_engine_battle_menus::battle_input::CommandPhase::CommitConfirm {
                    ..
                }) => press(&mut w, PadButton::Cross),
                Some(_) => press(&mut w, PadButton::Down),
            }
        }
        // Run the band out, watching for the frame the module's records are
        // first staged and counting every part seated over the whole band. A
        // module seated whole stages everything on the stager's first tick
        // (`SummonPhase::Armed`, retail's `0x801E4B1C`); one whose director
        // reports its spawns seats each record on the arm that calls
        // `FUN_80021B04`, and `tick_summon` may drain a scene whose parts have
        // all finished before a later arm seats into a fresh one - so the
        // total is summed over growth, restarting when the scene is replaced.
        let mut seen: Option<usize> = None;
        let mut seated = 0usize;
        let mut prev = 0usize;
        // Every record a part was seated from over the band, by its tag.
        let mut tags = std::collections::BTreeSet::new();
        // PROT 0908's slam record (seated on the fold's own pass) makes its
        // stager calls on the module's following ticks, after the outcome has
        // folded; a module whose parts call their stager is watched a few
        // frames past the fold.
        let stager_tail = seru_module_prot(picked).is_some_and(|e| matches!(e, 907 | 908));
        let mut tail = if stager_tail { 8 } else { 0 };
        for _ in 0..BAND_FRAMES {
            let now = w.casting.active_summon.as_ref().map(|s| s.parts.len());
            if seen.is_none() {
                seen = now;
            }
            let len = now.unwrap_or(0);
            if len < prev {
                prev = 0;
            }
            seated += len - prev;
            prev = len;
            if let Some(scene) = w.casting.active_summon.as_ref() {
                tags.extend(scene.parts.iter().filter_map(|p| p.tag));
            }
            if w.casting.pending_cast.is_none() {
                if tail == 0 {
                    break;
                }
                tail -= 1;
            }
            let _ = w.take_pending_summon_spawn();
            w.set_pad(0);
            frame(&mut w);
        }
        assert!(w.casting.pending_cast.is_none(), "cast {cast} never folded");
        // The id the menu actually committed, not the row we aimed at.
        let id = picked;
        assert_eq!(id, SERU_IDS[row], "row {row} is this cast's spell");
        let parts = seen
            .unwrap_or_else(|| panic!("cast {cast} (spell {id:#04x}) staged no cast-module scene"));
        staged.push((id, parts, seated, tags));
        // Clear the scene so the next cast's staging is its own.
        w.casting.active_summon = None;
    }

    assert_eq!(
        staged.len(),
        SERU_IDS.len(),
        "every player Seru spell cast and staged"
    );
    for (id, parts, seated, tags) in &staged {
        let entry = seru_module_prot(*id).unwrap();
        let module = pool.module(entry).unwrap();
        let stages_spawns = cmc::module_profile(entry).is_some_and(|p| p.stages_spawns);
        if !stages_spawns {
            let expect = module.parts.len();
            assert_eq!(
                *parts, expect,
                "spell {id:#04x} staged PROT {entry}'s whole record set"
            );
            assert!(*parts > 0);
            println!("[ok] spell {id:#04x} -> PROT {entry}: {parts} records staged");
            continue;
        }
        // A module whose director reports its spawns seats records the way
        // its own arms call `FUN_80021B04`, not all at once: PROT 0903 makes
        // three calls in arm 3 (`0x801F6E04..0x801F6E34`), three in arm 6
        // (`0x801F716C..0x801F719C`) and one in arm 8 (`0x801F72B8`). The
        // first staged frame holds arm 3's three, and the band seats all
        // seven by the time it folds.
        //
        // PROT 0905 makes four calls in arm 0, three in arm 4, two in arm 5,
        // two in arm 8 and five in arm 9 (`vera_direct`); its first staged
        // frame is arm 0's four. Its spawn stager `0x801F8078` - the move-VM
        // op-`0x20` hook arm 5's first glow calls - seats `0x801F8568` eight
        // at a time (`run_part_stager_calls`).
        //
        // PROT 0910, 0911 and 0913 report their arms' calls the same way
        // (`swordie_direct`, `orb_direct`, `nova_direct`) and their stagers
        // run off their parts' op-`0x20` calls too.
        //
        // PROT 0907's arms call `FUN_80021B04` (`nighto_direct`): two in arm
        // 4, one each in arms 5, 10 and 11. Its stager `0x801F7FA8` seats the
        // eight rays `0x801F8314` (arm 0) and the rings `0x801F8370`,
        // `0x801F8414`, `0x801F84B8` (arms 1..3), each off the part that
        // calls it (`run_part_stager_calls`).
        //
        // PROT 0908's arms call `FUN_80021B04` (`zenoir_direct`): seven in
        // arm 4, one in arm 6 and seven in arm 11. Its stager `0x801F8310`
        // seats `0x801F869C` (arm 0), `0x801F8880` and eight `0x801F8828`
        // (arm 1) and two `0x801F8ABC` (arm 2), off the calling parts too.
        // The static scan also recovers `0x801F90B0`, the one spawn of
        // `0x801F8EAC`: PROT 0904's stager, inherited residue in this image.
        //
        // PROT 0909 makes two calls in arm 2 and one in arm 7
        // (`viguro_direct`). Its stager `0x801F7AF4` is the one hook the band
        // runs (`run_viguro_stager_calls`), but its arms 2, 3, 4 and 6 - each
        // seating one record where the calling part stands - are called by
        // no part this band seats. Arm 11's three spawns on the camera's own
        // focus (`0x801F7FE0`, `0x801F8094`, `0x801F8158`) are not ported.
        //
        // PROT 0906 makes three calls in arm 4, one in arm 7 and two in arm
        // 12 (`gizam_direct`), and its stager `0x801F7740` runs off its
        // parts' op-`0x20` calls (`run_part_stager_calls`): arm 7's
        // `0x801F7EBC` calls stager arm 5 twelve times and arm 6 once,
        // arm 12's `0x801F7C44` calls arm 3 forty-eight times (eight passes
        // over the six bubble points), each seating one record.
        //
        // PROT 0912's body passes seat sixteen records in arm 2, fifteen in
        // arm 4, eight in arm 12 and ten in arm 14 (`freed_arm_spawns`); its
        // stager `0x801F835C` seats eight shards for arm 4's `0x801F8960`
        // (stager arm 3) and one record each for the five passes of arm
        // 12's `0x801F8D50` over stager arms 4 and 5.
        let (arms, hook_only, unseated): (Vec<&[cmc::ModuleSpawn]>, &[u32], &[u32]) = match entry {
            903 => (
                vec![
                    &cmc::GIMARD_SEAT_SPAWNS,
                    &cmc::GIMARD_TUNNEL_SPAWNS,
                    &cmc::GIMARD_BREATH_SPAWNS,
                ],
                &[],
                &[],
            ),
            905 => (
                vec![
                    &cmc::VERA_OPEN_SPAWNS,
                    &cmc::VERA_CUT_SPAWNS,
                    &cmc::VERA_SEAT_SPAWNS,
                    &cmc::VERA_CREATURE_SPAWNS,
                    &cmc::VERA_RESTORE_SPAWNS,
                ],
                &[0x801F_8568],
                &[],
            ),
            907 => (
                vec![
                    &cmc::NIGHTO_OPEN_SPAWNS,
                    &cmc::NIGHTO_CRYSTAL_SPAWNS,
                    &cmc::NIGHTO_SONG_SPAWNS,
                    &cmc::NIGHTO_RING_SPAWNS,
                ],
                &[0x801F_8314, 0x801F_8370, 0x801F_8414, 0x801F_84B8],
                &[],
            ),
            908 => (
                vec![
                    &cmc::ZENOIR_SEAT_SPAWNS,
                    &cmc::ZENOIR_STRIKE_SPAWNS,
                    &cmc::ZENOIR_SLAM_SPAWNS,
                ],
                &[0x801F_869C, 0x801F_8828, 0x801F_8880, 0x801F_8ABC],
                &[0x801F_90B0],
            ),
            906 => (
                vec![
                    &cmc::GIZAM_POOL_SPAWNS,
                    &cmc::GIZAM_SPRAY_SPAWNS,
                    &cmc::GIZAM_BUBBLE_SPAWNS,
                ],
                &[0x801F_7C8C, 0x801F_7E68, 0x801F_7F44],
                // Stager arm 4's record: no part this band seats calls arm 4.
                &[0x801F_7E1C],
            ),
            912 => (
                vec![
                    &cmc::FREED_CRYSTAL_SPAWNS,
                    &cmc::FREED_BURST_SPAWNS,
                    &cmc::FREED_FROST_SPAWNS,
                    &cmc::FREED_FIELD_SPAWNS,
                ],
                &[0x801F_88FC, 0x801F_8CF4, 0x801F_8DC8],
                &[],
            ),
            909 => (
                vec![&cmc::VIGURO_OPEN_SPAWNS, &cmc::VIGURO_CREATURE_SPAWNS],
                &[0x801F_81F4, 0x801F_8288, 0x801F_82CC, 0x801F_8310],
                &[0x801F_7FE0, 0x801F_8094, 0x801F_8158],
            ),
            // `0x801F8D0C` is stager arm 4's (called by arm 8's glow);
            // `0x801F8C7C` is the slash routine `FUN_801F81DC`'s hit spark
            // (`0x801F883C`), not reported; `0x801F90B0` is the one spawn of
            // `0x801F8EAC`, PROT 0904's stager, residue in this image.
            910 => (
                vec![&cmc::SWORDIE_RING_SPAWNS, &cmc::SWORDIE_GLOW_SPAWNS],
                &[0x801F_8A9C, 0x801F_8AF0, 0x801F_8BEC, 0x801F_8C34],
                &[0x801F_8C7C, 0x801F_8D0C, 0x801F_90B0],
            ),
            // `0x801F8190` is the opening's star swarm (arm 2's rand-spread
            // `FUN_80050ED4` loop and arm 3's mover): seated by the engine's
            // own port of that loop beside arm 2's calls, not reported by
            // the director.
            911 => (
                vec![
                    &cmc::ORB_FLASH_SPAWNS,
                    &cmc::ORB_RAY_SPAWNS,
                    &cmc::ORB_HEAL_SPAWNS,
                ],
                &[0x801F_832C, 0x801F_8498],
                &[0x801F_8190],
            ),
            913 => (
                vec![
                    &cmc::NOVA_VORTEX_SPAWNS,
                    &cmc::NOVA_ARM8_SPAWNS,
                    &cmc::NOVA_CLOUD_SPAWNS,
                    &cmc::NOVA_WHIRLWIND_SPAWNS,
                    &cmc::NOVA_LIFT_SPAWNS,
                    &cmc::NOVA_LANDING_SPAWNS,
                ],
                &[0x801F_8850, 0x801F_8914, 0x801F_8960],
                // The stager's arms 3..5 (`0x801F8E88` / `0x801F8FCC` /
                // `0x801F9088`) are called after the fold.
                &[0x801F_8E88, 0x801F_8FCC, 0x801F_9088],
            ),
            _ => panic!("PROT {entry} seats per arm but the test names no arm list"),
        };
        // PROT 0911's first staged frame also holds arm 2's sixteen stars.
        let swarm = if entry == 911 { 16 } else { 0 };
        assert_eq!(
            *parts,
            arms[0].len() + swarm,
            "spell {id:#04x}: PROT {entry}'s first staged frame is its first arm's calls"
        );
        let calls: usize = arms.iter().map(|a| a.len()).sum();
        // PROT 0907's and 0908's parts call their stager (op `0x20`), whose
        // arms seat more records off the calling part; every one of those
        // records is seated over the band, on top of the arms' own calls.
        let stager_run = matches!(entry, 905 | 906 | 907 | 908 | 910 | 912);
        // PROT 0911 and 0913 fold on their hit arms (9 / `0x13`), and a
        // body-paced director hands a pass's calls over on the next arm's
        // first tick, so this window closes before the later arms seat: it
        // holds at least the first arm's calls.
        if matches!(entry, 911 | 913) {
            assert!(
                *seated >= arms[0].len() + swarm,
                "spell {id:#04x}: PROT {entry} seats at least its first arm's calls ({seated})"
            );
        } else if stager_run {
            assert!(
                *seated > calls,
                "spell {id:#04x}: PROT {entry} seats its arms' {calls} calls and its stager's"
            );
            let missing: Vec<String> = hook_only
                .iter()
                .filter(|va| !tags.contains(va))
                .map(|va| format!("{va:#010x}"))
                .collect();
            assert!(
                missing.is_empty(),
                "spell {id:#04x}: PROT {entry}'s stager seated none of {missing:?}"
            );
            // PROT 0906 and 0912's stager seatings are counted exactly: 0906's
            // arm-7 timer calls stager arm 5 twelve times and arm 6 once and
            // its arm-12 timer calls arm 3 forty-eight times; 0912's
            // `0x801F8960` calls arm 3 once (eight shards) and `0x801F8D50`
            // calls arms 4 and 5 five times each.
            let hooked = match entry {
                906 => Some(13 + 48),
                912 => Some(8 + 10),
                _ => None,
            };
            if let Some(h) = hooked {
                assert_eq!(
                    *seated,
                    calls + h,
                    "spell {id:#04x}: PROT {entry} seats its arms' calls and its stager's"
                );
            }
        } else {
            assert_eq!(
                *seated, calls,
                "spell {id:#04x}: PROT {entry} seats every arm's spawn call over the band"
            );
        }
        // Per-arm seating drops none of the records the static spawn scan
        // recovers: each one is some arm's call.
        let called: Vec<u32> = arms
            .iter()
            .flat_map(|a| a.iter())
            .filter_map(|s| match s.record {
                cmc::SpawnRecord::Module(va) => Some(va),
                cmc::SpawnRecord::BattleProto(_) => None,
            })
            .collect();
        let unstaged: Vec<String> = module
            .parts
            .iter()
            .map(|p| {
                legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE
                    + p.record_off as u32
            })
            .filter(|va| !called.contains(va) && !hook_only.contains(va) && !unseated.contains(va))
            .map(|va| format!("{va:#010x}"))
            .collect();
        assert!(
            unstaged.is_empty(),
            "PROT {entry} records {unstaged:?} are staged by none of its arms"
        );
        println!(
            "[ok] spell {id:#04x} -> PROT {entry}: {parts} records on the first arm, \
             {seated} seated over the band (static set {})",
            module.parts.len()
        );
    }
}

/// The summon-creature directors' per-arm spawn tables
/// (`cast_module_camera::creature_arm_spawns`) were read off the
/// disassembly; every record they name parses as a record of the image
/// (most are ones its own spawn-site scan recovers), so a mistyped `a2`
/// would show here.
#[test]
fn every_creature_arm_spawn_names_a_record_of_its_module() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] creature arm spawns: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let pool = build_pool(&dir);
    let base = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
    for entry in cmc::CREATURE_SPAWN_ENTRIES
        .into_iter()
        .chain(cmc::CAPTURE_SPAWN_ENTRIES)
    {
        let m = pool.module(entry).expect("module loaded");
        let known: std::collections::BTreeSet<u32> =
            m.parts.iter().map(|p| base + p.record_off as u32).collect();
        let mut named = std::collections::BTreeSet::new();
        for phase in 0..=u8::MAX {
            for timing in [cmc::SpawnTiming::Pass, cmc::SpawnTiming::Every] {
                let capture = if timing == cmc::SpawnTiming::Pass {
                    cmc::capture_arm_spawns(entry, phase)
                } else {
                    cmc::capture_arm_spawns_every(entry, phase)
                };
                for s in cmc::creature_arm_spawns(entry, phase, timing)
                    .iter()
                    .chain(capture)
                {
                    let cmc::SpawnRecord::Module(va) = s.record else {
                        panic!("PROT {entry} arm {phase:#x}: a module record");
                    };
                    // A site whose `a2` the image scan cannot follow (a
                    // saved-register load) is parsed at its own offset, as
                    // the host's seating does.
                    let parsed = known.contains(&va)
                        || legaia_overlay_images::summon_overlay::parse_records_at(
                            &m.bytes,
                            &[(va - base) as usize],
                        )
                        .iter()
                        .any(|p| base + p.record_off as u32 == va);
                    assert!(
                        parsed,
                        "PROT {entry} arm {phase:#x}: {va:#010X} is no record of the image"
                    );
                    named.insert(va);
                }
            }
        }
        assert!(!named.is_empty(), "PROT {entry} spawns something");
        println!(
            "[ok] PROT {entry}: {} of {} records seated by its arms (the rest by part stagers)",
            named.len(),
            known.len()
        );
    }
}
