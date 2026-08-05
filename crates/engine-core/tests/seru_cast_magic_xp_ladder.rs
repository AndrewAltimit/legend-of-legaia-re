//! A Seru-magic cast crossing its **magic-XP threshold mid-battle** - the
//! ladder for the reach-triage `magic_xp.rs` row (`FUN_801F452C`, the
//! "magic level increased" banner) and the summon-cast request the same cast
//! stages.
//!
//! The chain under test is the live player path end to end: pad -> round
//! prompt -> command ring (Magic arm) -> spell submenu -> target ->
//! `World::cast_spell_on_slots` -> per-hit accrual
//! (`battle_formulas::summon_spell_xp_gain`, the `FUN_801DDB30` tail) ->
//! `World::accrue_summon_spell_xp` (`FUN_801E70BC` threshold walk over the
//! `0x8007656C` table) -> the level byte bump + the `FUN_801F452C` banner on
//! the world's banner channel -> the summon-spawn request the render hosts
//! consume (`take_pending_summon_spawn`).
//!
//! Disc-free: the clean-room Seru-magic catalog (`retail_magic`), a synthetic
//! threshold table in the retail shape, and the vanilla monster tables. Runs
//! in CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("magic_xp_ladder_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
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

fn wait_for_prompt(w: &mut World) -> bool {
    for _ in 0..0x400 {
        if w.battle_command.is_some() {
            return true;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    false
}

fn press(w: &mut World, b: PadButton) {
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    w.set_pad(0);
    let _ = w.tick();
}

#[test]
fn seru_cast_accrues_xp_and_crosses_its_level_threshold() {
    let mut w = build_world();
    // The clean-room Seru-magic catalog: id 0x81 = Gimard, a damage spell.
    w.spell_catalog = legaia_engine_core::retail_magic::retail_seru_magic_catalog();
    // The retail-shaped threshold curve (strictly ascending, level 1 needs
    // the total to EXCEED entry [0]).
    w.magic_xp_thresholds = Some([17, 50, 92, 144, 208, 288, 392, 536]);
    // Teach the acting character Gimard at level 1.
    {
        let rec = &mut w.roster.members[0];
        let mut list = rec.spell_list();
        list.count = 1;
        list.ids[0] = 0x81;
        list.levels[0] = 1;
        rec.set_spell_list(list);
    }

    enter_battle(&mut w);
    // Post-entry re-seat: caster MP + magic power, and a monster tanky enough
    // to survive several casts (a killed target ends the ladder early).
    for i in 0..3 {
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.mp = 99;
    }
    w.battle_magic[0] = 80;
    // Monster sizing matters to the accrual, and the shape is faithful: the
    // partial-hit gain is `damage * 12 / max_hp` (integer), so a huge target
    // accrues 0 per cast exactly as in retail. 300 max HP puts the ~195-damage
    // placeholder cast at ~7 XP for the first hit and the full 12 for the
    // kill hit - two casts cross the 17 threshold strictly.
    let ms = w.party_count as usize;
    w.actors[ms].battle.max_hp = 300;
    w.actors[ms].battle.hp = 300;
    // The enemy-side flee checkpoint (FUN_801EC0DC) can remove the monster on
    // its first pick under an unlucky seed; the scripted no-escape flag is
    // retail's own gate for it (ctx+0x287, tested at the roll's head).
    w.battle_no_escape = true;

    let rec_xp = |w: &World| legaia_engine_core::magic_xp::spell_xp(&w.roster.members[0], 0usize);
    assert_eq!(rec_xp(&w), 0);

    let mut leveled: Vec<(u8, u8, u8)> = Vec::new();
    let mut casts = 0usize;
    let mut summon_requests = 0usize;
    for _ in 0..12 {
        if !wait_for_prompt(&mut w) {
            panic!(
                "command session never reopened (cast {casts}); active={} state={:02X} spell_menu={} item_menu={} arts={} mode={:?} monster_hp={}",
                w.battle_ctx.active_actor,
                w.battle_ctx.action_state,
                w.battle_spell_menu.is_some(),
                w.battle_item_menu.is_some(),
                w.battle_arts_menu.is_some(),
                w.mode,
                w.actors[w.party_count as usize].battle.hp,
            );
        }
        // A round-open session sits on the `Begin | Run` prompt; a mid-round
        // one opens straight on the ring - dismiss the prompt when it is up.
        if matches!(
            w.battle_command.as_ref().map(|s| &s.phase),
            Some(legaia_engine_core::battle_input::CommandPhase::RoundPrompt { .. })
        ) {
            press(&mut w, PadButton::Cross); // Begin -> the command ring
        }
        // Only the caster's turn opens on slot 0; other members Spirit (the
        // ring's down arm - no damage) so the monster's HP budget is spent by
        // the casts alone and the accrual arithmetic stays deterministic.
        if w.battle_ctx.active_actor != 0 {
            press(&mut w, PadButton::Down); // ring: Spirit arm (turn consumed)
            continue;
        }
        press(&mut w, PadButton::Right); // ring: Magic arm
        assert!(
            w.battle_spell_menu.is_some(),
            "the Magic arm should open the spell submenu (cast {casts})"
        );
        press(&mut w, PadButton::Cross); // spell row 0 (Gimard) -> target
        press(&mut w, PadButton::Cross); // target confirm -> cast
        casts += 1;
        if w.take_pending_summon_spawn().is_some() {
            summon_requests += 1;
        }
        leveled.extend(w.drain_magic_level_ups());
        if !leveled.is_empty() {
            break;
        }
        assert!(
            rec_xp(&w) > 0,
            "cast {casts} accrued no spell XP (record +0x8 array untouched)"
        );
    }

    assert!(
        !leveled.is_empty(),
        "no level-up after {casts} casts; xp = {}",
        rec_xp(&w)
    );
    assert_eq!(leveled[0].0, 0, "caster ordinal");
    assert_eq!(leveled[0].1, 0x81, "the leveled spell");
    assert_eq!(leveled[0].2, 2, "level 1 -> 2");
    assert_eq!(
        w.roster.members[0].spell_list().levels[0],
        2,
        "the record's +0x161 level byte was bumped"
    );
    assert!(
        rec_xp(&w) > 17,
        "the accrued total strictly exceeded the level-1 threshold"
    );
    // Every cast requested the summon-creature spawn the render hosts consume.
    assert_eq!(
        summon_requests, casts,
        "each Seru cast stages exactly one summon spawn request"
    );
    // The FUN_801F452C banner: "<spell name>'s magic level increased." on the
    // world's banner channel, composed through the spell-name table.
    let name = w.spell_catalog.get(0x81).map(|d| d.name.clone()).unwrap();
    let banner = w
        .current_art_banner
        .as_ref()
        .expect("the level-up staged the retail banner");
    assert_eq!(
        banner.text,
        legaia_engine_core::magic_xp::magic_level_increased_message(&name)
    );
}
