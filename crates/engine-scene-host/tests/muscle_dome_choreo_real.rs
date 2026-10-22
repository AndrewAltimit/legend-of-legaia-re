//! Disc-gated: the Muscle Dome play-out closes to the battle's reach on every
//! rung of the course ladder.
//!
//! A dome leg is an ordinary battle, so a swing lands from where the range
//! law `FUN_8004E2F0` stops the approach - the attacker's live pair within
//! the size-class threshold of the target's body pair, offset by the party
//! reach `DAT_80078878`. The port once stopped the walk at a margin of its
//! own (both rest half-widths times 1.5 plus 120), which left a visible gap
//! between fist and monster on every rung. This walks every one of the 29
//! ladder rungs with the lead's battle form, both ways round, and asserts:
//!
//! - the attacker swings only once the range law reads zero, and the walk
//!   closed the seats' gap to get there;
//! - the defender reacts during the swing (the event-frame hit);
//! - a monster swings one of the picker's strike candidates (tags
//!   `0x0C..=0x1F` with a real AGL cost, `FUN_801E9FD4`), never its approach
//!   clips `0x20` / `0x21`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_battle_menus::muscle_dome_session::choreo::{
    BeatKind, BodyAct, ChoreoBody, DomeChoreo,
};
use legaia_engine_minigames::muscle_dome::DomePlay;
use legaia_engine_scene_host::scene_host::SceneHost;

fn play(attacker: usize) -> DomePlay {
    DomePlay {
        attacker,
        cmd: 0xC,
        power: 1,
        damage: 10,
        hp_after: [90, 90],
    }
}

#[test]
fn every_rung_closes_to_the_range_law_and_lands_its_hit() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let arena = host
        .index
        .entry_bytes_extended(legaia_engine_minigames::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32)
        .expect("arena overlay");
    let ladder = legaia_engine_minigames::muscle_dome::parse_course_ladder(&arena).expect("ladder");
    let player = host.index.entry_bytes_extended(863).expect("PLAYER1");
    let archive = host
        .index
        .entry_bytes_extended(867)
        .expect("monster archive");
    let mut rungs = 0;
    for course in &ladder {
        for rung in &course.rounds {
            let id = rung.monster_id as u16;
            let fighter = ChoreoBody::fighter(&player, 0).expect("Vahn's battle form");
            let monster = ChoreoBody::monster(&archive, id).expect("ladder monster decodes");
            let swing = monster.clips.swings[0]
                .as_ref()
                .expect("every ladder monster has a strike candidate");
            assert!(
                (0x0C..=0x1F).contains(&swing.action_id),
                "monster {id:#04x} swings entry tag {:#04x}, not a strike",
                swing.action_id
            );
            for attacker in 0..2 {
                let mut c = DomeChoreo::default();
                c.install([fighter.clone(), monster.clone()]);
                let gap0 = (c.position(1)[1] - c.position(0)[1]).abs();
                c.start(&[play(attacker)], true, [100, 100], [100, 100]);
                let mut swing_gap = None;
                let mut reacted = false;
                for _ in 0..1500 {
                    if !c.running() {
                        break;
                    }
                    c.tick();
                    if swing_gap.is_none()
                        && c.beat().is_some_and(|(b, _)| b.kind == BeatKind::Swing)
                    {
                        assert!(
                            c.in_reach(attacker),
                            "monster {id:#04x}: attacker {attacker} swings out of reach"
                        );
                        swing_gap = Some((c.position(1)[1] - c.position(0)[1]).abs());
                    }
                    reacted |= matches!(c.act(attacker ^ 1), BodyAct::Reaction(_));
                }
                let gap = swing_gap.expect("the turn reaches its swing");
                assert!(gap < gap0, "monster {id:#04x}: the walk closed nothing");
                assert!(reacted, "monster {id:#04x}: the defender never reacted");
                eprintln!("[rung] monster {id:#04x} attacker {attacker}: seats {gap0} -> {gap}");
            }
            rungs += 1;
        }
    }
    assert_eq!(rungs, 29, "the three courses carry 8 + 8 + 13 rungs");
}
