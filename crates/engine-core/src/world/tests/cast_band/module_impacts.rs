//! The slot-B modules' countdown-keyed tint stores read the word the pass
//! has already drained (`legaia_engine_cast::cast_module_impact`).

use super::*;
use legaia_engine_cast::cast_module_camera::SPEED_SCALAR;

/// PROT 0950's Rolling Flare (`0x5A`) on a monster caster at seat 3 and its
/// victim at seat 0.
fn rolling_flare_world() -> World {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut world = module_code_world();
    world.party.party_count = 3;
    world.battle_ctx.active_actor = 3;
    world.actors[3].battle_monster_id = Some(180);
    world.actors[3].battle.active_target = 0;
    for i in 0..8 {
        world.actors[i].battle.hp = 9000;
        world.actors[i].battle.max_hp = 9000;
    }
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[0x5A].class = CAPTURE_CLASS;
    entries[0x5A].sub_class = (950 - 935) as u8;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    assert_eq!(world.cast_module_for(0x5A), Some(950));
    world
}

/// Arm 3 drains its word (`sw v1,-0x7950(s0)` at `0x801F7ED8`) and then tests
/// the drained word against `scalar * 0x60` for the soft red
/// (`0x801F7F10..0x801F7F28`) and `scalar << 6` for the burn selector
/// (`0x801F7F30..0x801F7F48`). Each store lands on the tick whose drain takes
/// the word to its threshold, not a tick later.
#[test]
fn rolling_flare_burns_on_the_tick_its_drained_word_reaches_the_threshold() {
    let mut w = rolling_flare_world();
    w.arm_capture_cast_module(0x5A);
    let mut red_at = None;
    let mut burn_at = None;
    for tick in 0..20_000 {
        w.clock.display_frames += 1;
        let phase = w.casting.module_phase;
        let before = w.casting.module_cam.countdown.0;
        let busy = w.run_cast_module_code(0x5A, phase).is_some_and(|r| r.busy);
        if phase == 3 {
            let word = w.casting.module_cam.countdown.0;
            if red_at.is_none() && w.actors[0].battle.render_flag == 7 {
                red_at = Some((tick, before, word));
            }
            if burn_at.is_none() && w.actors[0].battle.impact_state == 1 {
                burn_at = Some((tick, before, word));
            }
        }
        if !busy || burn_at.is_some() {
            break;
        }
    }
    // The store lands on the tick whose drain crosses the threshold: the
    // word opened that tick above it and the tick left it at or under it.
    let (_, red_before, red_word) = red_at.expect("arm 3 tints the victim soft red");
    let (_, burn_before, burn_word) = burn_at.expect("arm 3 burns the victim");
    let red = SPEED_SCALAR * 0x60;
    let burn = SPEED_SCALAR << 6;
    assert!(
        red_before > red && red_word <= red,
        "soft red on the tick {red_before} -> {red_word}"
    );
    assert!(
        burn_before > burn && burn_word <= burn,
        "burn on the tick {burn_before} -> {burn_word}"
    );
}
