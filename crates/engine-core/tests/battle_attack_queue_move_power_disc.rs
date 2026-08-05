//! Disc-gated: the swing byte a confirmed Attack queues resolves a **real
//! move-power record**, which is the precondition the weapon-trail streak
//! pass hangs off.
//!
//! `World::step_actor_effect_script` keys the effect script's move-power
//! lookup on the acting actor's action-stream head byte
//! (`actor[+0x1DF]`, retail's `FUN_801DEA50` terminator sink). The lookup is
//! `legaia_engine_core::action_effect_script::move_power_record_offset` over
//! the id-index map at `0x801F4E63` (PROT 0898). An Attack that leaves that
//! byte at `0` resolves nothing, `MoveFxStreak` is never installed, and
//! `World::active_move_fx_trail_texpage` stays `None` - the streak pass emits
//! zero quads for the whole fight.
//!
//! This pins the two halves against the real table: the pre-fix value `0`
//! resolves to nothing, and the swing byte the command menu's confirm now
//! queues resolves to a record.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::action_effect_script::move_power_record_offset;
use legaia_engine_core::encounter::{
    EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
};
use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::is_swing_command;
use legaia_prot::archive::Archive;

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    None
}

fn overlay_0898(dir: &std::path::Path) -> Vec<u8> {
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry");
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read 0898");
    bytes
}

#[test]
fn the_queued_swing_byte_resolves_a_move_power_record() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ missing");
        return;
    };
    let catalog = MovePowerCatalog::from_overlay_0898(&overlay_0898(&dir)).expect("catalog");

    // Walk into an ordinary encounter - battle entry is what opens the
    // opening command session (same shape as `battle_player_driven.rs`).
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 30_000;
        w.actors[i].battle.max_hp = 30_000;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 120);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;

    let mut table = EncounterTable::new("attack_queue_move_power_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;

    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..8000 {
        w.set_pad(up);
        w.tick();
        if w.mode == SceneMode::Battle {
            break;
        }
    }
    assert!(w.battle_command.is_some(), "command session never opened");
    let slot = w.battle_ctx.active_actor as usize;

    // Walk retail's open flow to a committed swing: `Begin` on the round
    // prompt, then the ring's `Attack` arm (one Left onto the `Attack` arm it
    // opens on), then `Auto` on the attack-mode prompt, then the target
    // cursor's confirm. Phase-driven rather than a fixed press count so the
    // test says what it is steering, not how many frames that takes.
    use legaia_engine_core::battle_input::{BattleCommand, CommandPhase};
    let cross = InputState::mask_of([PadButton::Cross]);
    let left = InputState::mask_of([PadButton::Left]);
    let mut release = false;
    for _ in 0..256 {
        let Some(session) = w.battle_command.as_ref() else {
            break;
        };
        let pad = if release {
            0
        } else {
            match session.phase {
                CommandPhase::Menu { .. }
                    if session.menu_command() != Some(BattleCommand::Attack) =>
                {
                    left
                }
                _ => cross,
            }
        };
        release = !release;
        w.set_pad(pad);
        w.tick();
    }
    assert!(w.battle_command.is_none(), "Attack was never confirmed");

    // The effect script's move-power key is the stream head byte.
    let action = w.actors[slot].battle.params[0];
    assert!(
        is_swing_command(action),
        "the confirm must leave a swing command at the stream head, got {action:#04X}"
    );

    // Same base reconciliation `step_actor_effect_script` does: the catalog's
    // map is 0x801F4E63-based, the stepper reads the 0x801F4E64-based view.
    let map = catalog.id_index_map_bytes();
    let map = &map[1..];

    assert!(
        move_power_record_offset(map, 0).is_none(),
        "the pre-fix stream head (0) must resolve to no record - if it did, \
         this test could not tell the two states apart"
    );
    let offset = move_power_record_offset(map, action).unwrap_or_else(|| {
        panic!("queued swing {action:#04X} resolved no move-power record offset")
    });
    let index = offset / legaia_engine_core::action_effect_script::MOVE_POWER_STRIDE;
    assert!(
        index < catalog.len(),
        "swing {action:#04X} -> offset {offset} -> record {index} is past the \
         {}-record table",
        catalog.len()
    );
}
