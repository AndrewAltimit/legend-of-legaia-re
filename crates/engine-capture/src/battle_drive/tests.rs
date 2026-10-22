//! Unit tests for the battle drive.

use super::*;

#[test]
fn the_battle_drive_round_trips_through_its_env_form() {
    for d in [
        BattleDrive::Opening {
            swept: true,
            entry: 0xAF,
        },
        BattleDrive::Menu {
            flow: BattleFlowState::ArtsCommandEntry,
            seat: 2,
            spirit: 0,
        },
        BattleDrive::Menu {
            flow: BattleFlowState::CommitBegin,
            seat: 1,
            spirit: 0,
        },
        BattleDrive::Menu {
            flow: BattleFlowState::CategoryMenu,
            seat: 2,
            spirit: 0b11,
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x6F,
            category: 2,
            queued: 0x7A,
            spare: false,
            absorbed: 0,
            end: SpanGate::Exit { phase: 3 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x52,
            category: 3,
            queued: 0x0F,
            spare: true,
            absorbed: 1,
            end: SpanGate::DoneHold { timer: -1 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x5A,
            category: 3,
            queued: 0x0D,
            spare: true,
            absorbed: 0,
            end: SpanGate::Results { hold: 80 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x6F,
            category: 2,
            queued: 0xAD,
            spare: false,
            absorbed: 0,
            end: SpanGate::CaptureFade {
                height: 0xFF40,
                accum: 344,
                arm: Some((1, 496)),
                yaw: Some(0x83C),
            },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x19,
            category: 3,
            queued: 0x08,
            spare: false,
            absorbed: 0,
            end: SpanGate::Age { accum: 552 },
            style: Some(3),
            steer: ActionSteer {
                target: Some(4),
                yaw: Some(0xA98),
                message: Some((0x801C_ED18, 25)),
                plate_cleared: true,
                ..ActionSteer::default()
            },
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x20,
            category: 3,
            queued: 0x0F,
            spare: false,
            absorbed: 0,
            end: SpanGate::Age { accum: 176 },
            style: Some(0),
            steer: ActionSteer {
                yaw: Some(0x280),
                arts: true,
                gauge: Some(153),
                clip: Some(0x11),
                aim: Some(1),
                queue: Some([
                    0x0D, 0x0F, 0x0E, 0x19, 0x27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ]),
                cursor: Some(5),
                fading: Some((3, 0xF0)),
                spoils: true,
                park: true,
                ..ActionSteer::default()
            },
        },
    ] {
        assert_eq!(BattleDrive::from_env(&d.to_env()), Some(d));
    }
    assert_eq!(BattleDrive::from_env("menu,40"), None);
}

/// A committed arts queue reads back as the arrows that were entered:
/// swings as they stand, each starter + art pair as its art's last arrow.
/// `battle_melee_hit_spark`'s `0D 0F 0E 19 27` is Right Up Down Up when
/// art `0x27` is Up Down Up; a constant with no known combo (a Super
/// tail) or a stray byte leaves it unrecoverable.
#[test]
fn entered_arrows_undo_the_queue_builder() {
    use legaia_art::Command::{Down, Up};
    let combo = |a: u8| (a == 0x27).then(|| vec![Up, Down, Up]);
    let mut q = [0u8; 16];
    q[..5].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x19, 0x27]);
    let mut want = [0u8; 16];
    want[..4].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x0F]);
    assert_eq!(entered_arrows(&q, combo), Some(want));
    q[3] = 0x1A; // a newly-learned starter reads the same
    assert_eq!(entered_arrows(&q, combo), Some(want));
    q[4] = 0x2B;
    assert_eq!(entered_arrows(&q, combo), None, "unknown art");
    assert_eq!(entered_arrows(&[0u8; 16], combo), None, "empty queue");
    let mut stray = [0u8; 16];
    stray[0] = 0x2B;
    assert_eq!(entered_arrows(&stray, combo), None, "bare constant");
}

/// Taking an absorbed Seru back off a list undoes the Done band's
/// prepend exactly - ids, levels and the XP words - and leaves a list
/// without it alone.
#[test]
fn unlearning_undoes_the_absorb_grant() {
    use legaia_engine_battle::magic_xp::learn_spell_prepend;
    let mut rec = legaia_save::CharacterRecord::zeroed();
    learn_spell_prepend(&mut rec, 0x83);
    learn_spell_prepend(&mut rec, 0x85);
    rec.raw[0x8..0xC].copy_from_slice(&7u32.to_le_bytes());
    rec.raw[0xC..0x10].copy_from_slice(&9u32.to_le_bytes());
    let before = rec.raw.clone();
    learn_spell_prepend(&mut rec, 0x81);
    unlearn_spell(&mut rec, 0x81);
    assert_eq!(rec.raw, before);
    unlearn_spell(&mut rec, 0x8A);
    assert_eq!(rec.raw, before);
}

#[test]
fn a_monster_pool_slot_maps_onto_the_engine_seating() {
    assert_eq!(engine_seat(1, 2), 1);
    assert_eq!(engine_seat(3, 1), 1);
    assert_eq!(engine_seat(4, 2), 3);
    assert_eq!(engine_seat(3, 3), 3);
}

/// A parked-approach capture ([`ActionSteer::park`]) holds the acting
/// monster's walk from the Attack arm to the poll and, in the poll, stages
/// idle over the walk - the dead Move clip retail parks on. Without the
/// flag neither happens.
#[test]
fn a_park_steer_holds_the_walk_and_stages_idle_in_the_poll() {
    let run = |park: bool, state: u8| {
        let mut w = legaia_engine_core::world::World::new();
        w.enter_battle(1, 1);
        let monster = usize::from(engine_seat(3, 1));
        w.battle_ctx.active_actor = monster as u8;
        w.battle_ctx.action_state = state;
        w.actors[monster].battle.queued_anim = 1;
        let drive = BattleDrive::Action {
            seat: 3,
            state: 0x19,
            category: 3,
            queued: 0x08,
            spare: false,
            absorbed: 0,
            end: SpanGate::Age { accum: 552 },
            style: None,
            steer: ActionSteer {
                park,
                ..ActionSteer::default()
            },
        };
        drive.steer(&mut w);
        (
            w.battle.approach_hold,
            w.actors[monster].battle.queued_anim,
            monster as u8,
        )
    };
    let (hold, queued, monster) = run(true, 0x19);
    assert_eq!(hold, Some(monster));
    assert_eq!(queued, 0, "the poll shows the dead clip's idle");
    let (hold, queued, monster) = run(true, 0x14);
    assert_eq!(hold, Some(monster), "the Attack arm already holds the walk");
    assert_eq!(queued, 1, "the walk is staged ahead of the poll");
    let (hold, queued, _) = run(false, 0x19);
    assert_eq!(hold, None);
    assert_eq!(queued, 1);
}
