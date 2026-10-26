//! Disc-gated: a Muscle Dome leg is fought as the **ordinary battle**.
//!
//! The arena's fight start `FUN_801D1510` writes the ladder rung's monster
//! into the formation cell `DAT_8007BD0C`, `DAT_8007BD60 = 0x80`, the map id
//! `_DAT_80084540 = 0x4C6` (`other6`) and game mode `0x14`; the battle exit
//! selector `FUN_80046A20` sends a `0x100`-word battle back to the arena
//! (mode `0x18`). Driven here through the door warp and the shared host tick:
//!
//! 1. the hub's first visit plays with the leg not yet in battle;
//! 2. the hand-off opens `SceneMode::Battle` against the rung's monster,
//!    with the lead alone, the arena backdrop as the stage and the arena's
//!    special-battle word up;
//! 3. a knockout returns to the arena hub between legs, the contest one rung
//!    on, and no game-over or field return on the way.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_mode::minigame_entry::MinigameSubId;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::{MinigameWarpOutcome, SceneHost};

#[test]
fn a_leg_is_an_ordinary_battle_that_returns_to_the_arena() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let mut party = legaia_save::Party::zeroed(1);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_max = 999;
    hms.hp_cur = 999;
    party.members[0].set_hp_mp_sp(hms);
    host.world.load_party(party);

    host.world.minigames.pending_warp = Some(MinigameSubId::MuscleDome.sub_id());
    assert!(matches!(
        host.drain_minigame_warp(),
        Some(MinigameWarpOutcome::Entered(MinigameSubId::MuscleDome))
    ));
    let rung = host
        .world
        .minigames
        .muscle_battle_monster
        .expect("the door stages the rung's monster for the leg's battle");
    let round0 = host
        .world
        .minigames
        .muscle_contest
        .as_ref()
        .map(|c| c.round());

    // 1. The first visit owns the frame; the leg is not a battle yet.
    let mut first_visit_seen = false;
    let mut entered_at = None;
    for f in 0..3000u32 {
        let _ = host.tick();
        first_visit_seen |= host.world.minigames.muscle_hub.first_visit.is_some();
        if host.world.mode == SceneMode::Battle {
            entered_at = Some(f);
            break;
        }
        assert!(
            !host.world.muscle_leg_in_battle(),
            "frame {f}: the battle opened under a hub screen"
        );
    }
    assert!(first_visit_seen, "the first visit plays ahead of the fight");
    let f = entered_at.expect("the hub hands the leg to the battle");

    // 2. The leg's battle: the rung's monster against the lead alone, on the
    //    arena backdrop, under the arena's special-battle word.
    assert!(host.world.muscle_leg_in_battle());
    let monsters = host.world.battle_monster_slots();
    assert_eq!(monsters.len(), 1, "one opponent: {monsters:?}");
    assert_eq!(monsters[0].1, rung, "the opponent is the ladder rung");
    assert_eq!(host.world.party.party_count, 1, "the lead fights alone");
    assert_ne!(host.world.special_battle_word(), 0);
    let stage = host.battle_stage_entry();
    let arena_stage = host.index.battle_stage_entry_for_scene("other6");
    assert!(stage.is_some() && stage == arena_stage, "stage {stage:?}");
    eprintln!("[ok] battle opened at frame {f} vs {rung:#04x} on stage {stage:?}");

    // 3. Knock the opponent out and let the battle end itself.
    let slot = monsters[0].0;
    host.world.actors[slot].battle.hp = 0;
    let mut back = None;
    for g in 0..6000u32 {
        host.world.set_pad(if g % 20 == 0 {
            legaia_engine_system::input::PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.tick();
        if host.world.mode != SceneMode::Battle {
            back = Some(g);
            break;
        }
    }
    host.world.set_pad(0);
    assert!(back.is_some(), "the knocked-out leg leaves the battle");
    assert_eq!(host.world.mode, SceneMode::MuscleDome, "back in the arena");
    assert!(!host.world.game_over);
    assert!(!host.world.muscle_leg_in_battle());
    assert!(
        host.world.muscle_hub_between_legs(),
        "a won leg on an open course re-enters the hub"
    );
    assert_eq!(host.world.special_battle_word(), 0);
    let round1 = host
        .world
        .minigames
        .muscle_contest
        .as_ref()
        .map(|c| c.round());
    assert_eq!(round1, round0.map(|r| r + 1), "the ladder moved one rung");
    eprintln!("[ok] leg returned to the hub after {back:?} ticks, round {round1:?}");
}
