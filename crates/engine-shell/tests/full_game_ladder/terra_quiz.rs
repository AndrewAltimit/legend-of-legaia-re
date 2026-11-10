//! Terra's Snowdrift Cave walk: her first exit on a talk, then the quiz stops
//! a walk-on engages. Every one of her stops is her own record moving her, so
//! the talk and the touch must both run it on her context. Disc-gated.

use super::*;

/// Terra's placement record, `cave01` `P1[2]`.
const TERRA: u8 = 2;

/// Page the dialogue up, answering a picker with option `answer`, until
/// control has been back for a moment.
fn play_out(session: &mut NativeSession, answer: usize) {
    let mut idle = 0;
    let mut revealed = false;
    let mut on_picker = false;
    for t in 0..7000 {
        let w = &session.host.world;
        let panel = w
            .dialog
            .inline
            .as_ref()
            .and_then(|i| i.panel.as_ref())
            .or(w.script_dialog_panel())
            .map(|p| {
                (
                    p.picker().is_some(),
                    p.picker_cursor(),
                    p.is_waiting_for_input(),
                )
            });
        let tap = t % 24 < 2;
        let pad = match panel {
            Some((true, cursor, waiting)) => {
                if !on_picker {
                    on_picker = true;
                    revealed = false;
                }
                if !tap {
                    0
                } else if !waiting || !revealed {
                    revealed |= waiting && t % 24 == 1;
                    PadButton::Cross.mask()
                } else if cursor < answer {
                    PadButton::Down.mask()
                } else {
                    PadButton::Cross.mask()
                }
            }
            Some(_) => {
                on_picker = false;
                if tap { PadButton::Cross.mask() } else { 0 }
            }
            None => 0,
        };
        idle = if released(session) { idle + 1 } else { 0 };
        if idle > 200 {
            return;
        }
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
    }
    panic!("the conversation never handed control back");
}

/// Step from `from` in `step` onto a walk-on tile, then play out what it
/// starts.
fn walk_on(session: &mut NativeSession, from: (u8, u8), step: (i16, i16), answer: usize) {
    session.host.world.seat_player_at_tile(from.0, from.1);
    for _ in 0..8 {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
    }
    let pad = pad_for_step(session, step.0, step.1);
    for _ in 0..50 {
        session.host.world.set_pad(pad);
        session.tick().expect("tick");
        if !released(session) {
            break;
        }
    }
    session.host.world.set_pad(0);
    assert!(!released(session), "the walk-on at {from:?} fired");
    play_out(session, answer);
}

fn terra_tile(session: &NativeSession) -> (i16, i16) {
    let (x, z) = session.host.world.npcs.positions[&TERRA];
    (x >> 7, z >> 7)
}

#[test]
fn terra_walks_ahead_to_each_quiz_stop() {
    let Some(inp) = inputs() else { return };
    let mut session = open_session(&inp.extracted);
    session
        .enter_scene_live("cave01", &live_opts(false))
        .expect("enter cave01");
    session.host.world.party.active_party = vec![1];
    session.host.world.party.party_leader_slot = Some(1);
    for _ in 0..700 {
        session.host.world.set_pad(0);
        session.tick().expect("tick");
    }

    // The talk: her own `22 09` plays the walk clip as she leaves.
    let (nx, nz) = session.host.world.npcs.positions[&TERRA];
    let w = &mut session.host.world;
    let player = usize::from(w.player_actor_slot.expect("a player"));
    w.actors[player].move_state.world_x = nx - 120;
    w.actors[player].move_state.world_z = nz - 120;
    w.face_player_sector(5);
    assert_eq!(w.field_interact_probe_slot(), Some(TERRA));
    assert!(!w.npc_clip_cursor_bound(TERRA));
    w.set_pad(PadButton::Cross.mask());
    session.tick().expect("tick");
    session.host.world.set_pad(0);
    play_out(&mut session, 0);
    assert!(
        session.host.world.npc_clip_cursor_bound(TERRA),
        "her walk-off clip played"
    );
    assert_eq!(terra_tile(&session), (121, 44));

    // The stops the walk-ons engage.
    walk_on(&mut session, (121, 42), (0, -1), 0);
    assert_eq!(terra_tile(&session), (104, 56));
    walk_on(&mut session, (105, 55), (0, -1), 2);
    assert_eq!(terra_tile(&session), (79, 66), "after the first question");
    walk_on(&mut session, (81, 66), (0, -1), 1);
    assert_eq!(terra_tile(&session), (59, 44), "after the second question");
    walk_on(&mut session, (60, 46), (0, -1), 3);
    assert_eq!(terra_tile(&session), (68, 103), "after the third question");
}
