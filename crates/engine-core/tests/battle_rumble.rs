//! Disc-free: an encounter and the fight after it rumble the controller the
//! way retail does, through the live session (`World::tick`), and the
//! options screen's Encounters / Battles vibration rows silence each half.
//!
//! The retail reference is a PCSX-Redux capture of the `rim_elm_queen_bee_battle`
//! state (`scripts/pcsx-redux/autorun_rumble_words.lua`): the encounter
//! handoff holds both words at `0xFF` for six vsyncs before the intro zeroes
//! them; a monster's landed swing on the party stores `0xFF` with decay rate
//! `0x20` and the battle frame driver takes `step * 0x20` off it a pass
//! (`0xFF -> 0x9F -> 0x3F -> 0` on three-vsync passes); the effect script's
//! floor part stores `0x80` at rate `0xC`.

use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::rumble::RumbleMotors;

/// A three-member party seated against one sturdy, hard-hitting monster,
/// still in the field with the scripted entry armed.
fn armed_world(battles_off: bool, encounters_off: bool) -> World {
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(1, "Biter", 4000, 60);
    def.udf = 20;
    def.ldf = 20;
    cat.insert(def);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(1)]));

    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 900;
        hms.hp_max = 900;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 900;
        w.actors[i].battle.max_hp = 900;
        w.actors[i].battle.liveness = 1;
        w.battle.set_battle_attack(i as u8, 24);
        w.battle.set_battle_defense(i as u8, 16);
    }
    w.set_formation_table(table, cat);
    w.toggles.retail_options.vibration_battles = u8::from(battles_off);
    w.toggles.retail_options.vibration_encounters = u8::from(encounters_off);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1), "formation row 1 registers");
    w
}

/// Tick until the mode flips to battle, recording the motor words after
/// every tick.
fn run_transition(w: &mut World) -> Vec<RumbleMotors> {
    let mut words = Vec::new();
    for _ in 0..400 {
        w.tick();
        words.push(w.rumble.motors);
        if w.mode == SceneMode::Battle {
            return words;
        }
    }
    panic!("the intro never handed off to the battle");
}

#[test]
fn an_encounter_jolts_for_the_load_window_then_the_intro_zeroes() {
    let mut w = armed_world(false, false);
    let words = run_transition(&mut w);
    let full = RumbleMotors {
        pulse: 0xFF,
        level: 0xFF,
    };
    let jolt = words.iter().take_while(|m| **m == full).count();
    assert_eq!(jolt, 6, "six vsyncs at full strength, as retail: {words:?}");
    assert!(
        words[jolt..].iter().all(|m| *m == RumbleMotors::default()),
        "the intro holds both words at zero through the spin"
    );
    // The payload the host reads follows the words.
    let mut w = armed_world(false, false);
    w.tick();
    assert!(w.rumble_frame().strong > 0.99);
}

#[test]
fn encounters_off_keeps_the_transition_still() {
    let mut w = armed_world(false, true);
    let words = run_transition(&mut w);
    assert!(words.iter().all(|m| *m == RumbleMotors::default()));
}

/// One battle frame of the decay, as `FUN_80046A20` runs it.
fn decayed(m: RumbleMotors, rp: i32, rl: i32, step: i32) -> RumbleMotors {
    let f = |w: i32, r: i32| {
        if w > 0 {
            (w - step * r).clamp(0, 0xFF)
        } else {
            w.max(0)
        }
    };
    RumbleMotors {
        pulse: f(m.pulse, rp),
        level: f(m.level, rl),
    }
}

/// Every tick that changes a word either runs it down by the stored rates on
/// a battle pass, or stores one of the battle overlay's writes (seen at its
/// own value, or already one pass into its decay when the store and the pass
/// share a tick); a party swing landing on the monster is among the stores.
#[test]
fn a_fight_rumbles_on_landed_hits_and_the_frame_driver_runs_it_down() {
    use legaia_engine_system::rumble::{BattleRumbleWrite, battle_writes as bw};
    let known = [
        bw::FINISHER_PARTY_STRUCK,
        bw::FINISHER_MONSTER_STRUCK,
        bw::FLOOR_PART,
        bw::BLOCK_PARTY_DEFENDS,
        bw::BLOCK_PARTY_ATTACKS,
        bw::HIT_PARTY_STRUCK,
        bw::HIT_PARTY_ATTACKS,
        bw::ROT_PARTY_STRUCK,
    ];
    let mut w = armed_world(false, false);
    run_transition(&mut w);
    let mut prev = w.rumble.motors;
    let mut decays = 0u32;
    let mut stores: Vec<BattleRumbleWrite> = Vec::new();
    for _ in 0..6000 {
        w.tick();
        let now = w.rumble.motors;
        let (rp, rl) = (
            w.rumble.battle_decay.pulse_rate,
            w.rumble.battle_decay.level_rate,
        );
        let step = i32::from(w.battle.anim_pass_vsyncs);
        if now != prev {
            if step > 0 && now == decayed(prev, rp, rl, step) {
                decays += 1;
            } else {
                let found = known.iter().copied().find(|k| {
                    let at = RumbleMotors {
                        pulse: k.pulse,
                        level: k.level,
                    };
                    (k.pulse_rate, k.level_rate) == (rp, rl)
                        && (now == at || now == decayed(at, rp, rl, step))
                });
                match found {
                    Some(k) => stores.push(k),
                    None => panic!(
                        "{prev:?} -> {now:?} (rates {rp}/{rl}, step {step}) is neither a \
                         store nor the decay"
                    ),
                }
            }
        }
        prev = now;
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert!(decays > 0, "the words never decayed");
    assert!(
        stores.contains(&bw::HIT_PARTY_ATTACKS),
        "no landed party swing among {stores:?}"
    );
}

#[test]
fn battles_off_keeps_the_fight_still() {
    let mut w = armed_world(true, false);
    run_transition(&mut w);
    for _ in 0..6000 {
        w.tick();
        assert_eq!(w.rumble.motors, RumbleMotors::default());
        if w.mode != SceneMode::Battle {
            break;
        }
    }
}
