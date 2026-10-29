//! Disc-gated: the Rebirth 1F training gauntlet (`tower_gauntlet`: two
//! waves, then six refill arrivals with three standing at most) fought on
//! auto through the native session on its pack map resolves, with no party
//! member parked in an approach state while refills take freed seats.
//!
//! The play-test defect: a refill seated on the monster wipe left the action
//! SM to step `EndOfAction -> PreActionWait` on its own, re-seeding the member
//! that had just acted at the corpse it had killed (a seat the refill did not
//! take), and the approach (`0x19`) never came within reach.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_battle_vm::battle_action::ActionState;
use legaia_engine_cheats::cheats::{PlayerCheat, WorldCheats};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_expansion_format::ExpansionPack;

const MAP_ID: &str = "rebirth_tower_1f";
/// Longest a real approach walk takes (see `dead_target_redirect_disc`).
const APPROACH_HOLD_LIMIT: u32 = 900;
const BATTLE_TICKS: u32 = 120_000;

fn open() -> Option<NativeSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    let Some(extracted) = extracted else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let files = legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read");
    let pack = ExpansionPack::from_files(&files).expect("the Rebirth pack parses");
    session
        .host
        .activate_expansion_pack(Arc::new(pack))
        .expect("the Rebirth pack activates");
    Some(session)
}

#[test]
fn the_refill_gauntlet_resolves_with_no_approach_park() {
    // Full strength wipes an auto-battling party; these scales leave the
    // refills standing long enough to overlap the party's turns.
    for permille in [100u16, 400] {
        for seed in [1u32, 0x1234_5678, 0xDEAD_BEEF] {
            if !run(permille, seed) {
                return;
            }
        }
    }
}

fn run(permille: u16, seed: u32) -> bool {
    eprintln!("== permille {permille} seed {seed:#x}");
    let Some(mut session) = open() else {
        return false;
    };
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: false,
        battle_bgm: None,
    };
    session
        .enter_expansion_map_live(MAP_ID, "", &opts)
        .expect("enter the pack map");
    let w = &mut session.host.world;
    w.set_active_party(vec![0, 1, 2]);
    w.apply_cheat(&PlayerCheat::PartyLevel(99), None);
    w.apply_cheat(&PlayerCheat::Restore, None);
    w.toggles.enemy_stat_permille = permille;
    w.rng_state = seed;
    assert!(w.start_expansion_battle("tower_gauntlet"));

    let mut in_battle = false;
    let mut hold = 0u32;
    let mut seen: Vec<(u32, usize, u16)> = Vec::new();
    let mut alive = [false; 8];
    let mut trace: Vec<String> = Vec::new();
    let mut last_key = (0u8, 0u8, 0u8);
    let mut last_hp = [0u16; 8];
    for tick in 0..BATTLE_TICKS {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
        let w = &session.host.world;
        if w.mode == SceneMode::Battle {
            in_battle = true;
            for (s, was) in alive.iter_mut().enumerate() {
                let a = &w.actors[s];
                let now = a.active && a.battle.liveness != 0 && a.battle_monster_id.is_some();
                if now && !*was {
                    seen.push((tick, s, a.battle_monster_id.unwrap()));
                }
                *was = now;
            }
            let aa = w.battle_ctx.active_actor;
            let key = (
                w.battle_ctx.action_state,
                aa,
                w.actors
                    .get(aa as usize)
                    .map_or(0, |a| a.battle.active_target),
            );
            if key != last_key {
                last_key = key;
                trace.push(format!(
                    "t{tick} state {:?} actor {} tgt {}",
                    ActionState::from_byte(key.0),
                    key.1,
                    key.2
                ));
            }
            for (s, last) in last_hp.iter_mut().enumerate() {
                let a = &w.actors[s];
                let hp = a.battle.hp;
                if hp != *last {
                    if hp == 0 || *last == 0 {
                        trace.push(format!(
                            "t{tick} slot {s} hp {last} -> {hp} mon {:?} seat {:?} pos {:?}",
                            a.battle_monster_id,
                            a.battle.seat,
                            (a.move_state.world_x, a.move_state.world_z)
                        ));
                    }
                    *last = hp;
                }
            }
            let state = ActionState::from_byte(w.battle_ctx.action_state);
            let approaching = matches!(
                state,
                Some(
                    ActionState::AttackShortStep
                        | ActionState::AttackAdvance
                        | ActionState::AttackWindup
                )
            );
            hold = if approaching { hold + 1 } else { 0 };
            if hold > APPROACH_HOLD_LIMIT {
                for l in trace.iter().rev().take(60).rev() {
                    eprintln!("{l}");
                }
                let a = w.battle_ctx.active_actor as usize;
                let act = &w.actors[a];
                panic!(
                    "approach state {state:?} held {hold} frames at tick {tick} \
                     (actor {a} target {:?} pos {:?}); arrivals {seen:?}; \
                     monsters {:?}",
                    act.battle.active_target,
                    (act.move_state.world_x, act.move_state.world_z),
                    (0..8)
                        .map(|s| (
                            s,
                            w.actors[s].active,
                            w.actors[s].battle.liveness,
                            w.actors[s].battle.hp,
                            w.actors[s].battle_monster_id,
                            w.actors[s].battle.seat,
                            (
                                w.actors[s].move_state.world_x,
                                w.actors[s].move_state.world_z
                            )
                        ))
                        .collect::<Vec<_>>()
                );
            }
        } else if in_battle {
            eprintln!("[ran] tower_gauntlet resolved after {tick} ticks; arrivals {seen:?}");
            assert_eq!(seen.iter().filter(|s| s.1 >= 3).count(), 2 + 3 + 6);
            return true;
        }
        assert!(
            !w.game_over && !w.game_over_hold,
            "party wiped at tick {tick}"
        );
    }
    let w = &session.host.world;
    for s in 0..8 {
        let a = &w.actors[s];
        eprintln!(
            "slot {s}: active {} live {} hp {}/{} mon {:?} tgt {} seat {:?} pos {:?} anim {}/{} flags {:#x}",
            a.active,
            a.battle.liveness,
            a.battle.hp,
            a.battle.max_hp,
            a.battle_monster_id,
            a.battle.active_target,
            a.battle.seat,
            (a.move_state.world_x, a.move_state.world_z),
            a.battle.current_anim,
            a.battle.queued_anim,
            a.battle.flag_word
        );
    }
    eprintln!(
        "action_state {:?} active_actor {} levels {:?}",
        ActionState::from_byte(w.battle_ctx.action_state),
        w.battle_ctx.active_actor,
        w.party.level_up_tracker.level
    );
    panic!("tower_gauntlet unresolved after {BATTLE_TICKS} ticks; arrivals {seen:?}");
}
