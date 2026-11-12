//! Baka Fighter's cabinet state machine `FUN_801CF388` (PROT 0976), frame
//! by frame from its boot state over whole sessions on a generated pad,
//! against `BakaCabinet::tick`: every scalar the port keeps, read through
//! `BakaCabinet::retail_words` against the overlay word it mirrors, and the
//! cue the frame leaves in the ring slot `0x8007B6D8`.
//!
//! Every call out of the machine's body is stubbed: its widget, banner and
//! HUD draws, the actor spawns, the fight's round SM and the return warp.
//! What the stubbed round SM writes the harness writes in its place, after
//! the machine's frame: the round timer `DAT_801DBF88` steps by the frame step
//! once a round is decided (`CabinetInput::round_clock`), and the deciding
//! fighter's round wins `DAT_801DBFF0` / `DAT_801DC098` rise. The duel camera
//! the banner arms move runs beside the port as the fight host runs it
//! (`DuelCamera`), the cabinet's own arm moving retail's. The scene-ready
//! bit `0x1F800394 & 0x80` the round-ready state waits on stands up.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_cabinet::{
    BakaCabinet, CabinetInput, ST_DUEL, ST_EXIT, ST_ROUND_BANNER_A, ST_ROUND_BANNER_B,
    ST_ROUND_BANNER_C, ST_ROUND_SETUP, SecretOpponent,
};
use legaia_engine_minigames::baka_duel::DuelCamera;
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CABINET: u32 = 0x801C_F388;
const CABINET_END: u32 = 0x801D_21FC;
const ACTOR: u32 = 0x8012_0000;
const FRAME_STEP: u32 = 0x1F80_0393;
const EDGE: u32 = 0x8007_B874;
const HELD: u32 = 0x8007_B850;
const EDGE_ALT: u32 = 0x8007_B938;
const DEV_MENU: u32 = 0x8007_B868;
const CUE: u32 = 0x8007_B6D8;
const PLAYER_WINS: u32 = 0x801D_BFF0;
const OPPONENT_WINS: u32 = 0x801D_C098;
const WIN_TARGET: u32 = 0x801D_BED0;
const ROUND_TIMER: u32 = 0x801D_BF88;
/// The opponent's roster id and the roster table its prize is read from
/// (`0x801D06D4..0x801D06FC`).
const ROSTER_ID: u32 = 0x801D_BF74;
const ROSTER: u32 = 0x801D_769C;
const SECRET: u32 = 0x801D_BF06;
/// The duel's pause gate the combat ticks leave (`DAT_801DBEC4`).
const PAUSE_GATE: u32 = 0x801D_BEC4;
const TALLY_DRAIN: u32 = 0x801D_239C;

fn word(cpu: &mut psx_retail_exec::Cpu, a: u32) -> i32 {
    if a == SECRET {
        i32::from(cpu.mem.rd16(a) as i16)
    } else {
        cpu.mem.rd32(a) as i32
    }
}

#[test]
fn the_cabinet_runs_sessions_as_retail_does() {
    differential!(prot(976, CABINET));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    cpu.stub_outside = Some((CABINET, CABINET_END));
    // The score tally's answer, scripted: rows still draining for a while
    // after each match.
    let draining = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let draining = draining.clone();
        cpu.hook(TALLY_DRAIN, move |r, _| {
            r.r[psx_retail_exec::V0] = u32::from(draining.get())
        });
    }
    let mut rng = Rng(0x4341_4249);
    let mut seen = std::collections::BTreeSet::new();
    let mut frames_run = 0usize;
    for session in 0..120 {
        cpu.mem.load(OVERLAY_BASE, &overlay);
        for o in (0..0x100).step_by(4) {
            cpu.mem.wr32(ACTOR + o, 0);
        }
        let mut port = BakaCabinet::new();
        for (a, v) in port.retail_words() {
            if a == SECRET {
                cpu.mem.wr16(a, v as u16);
            } else {
                cpu.mem.wr32(a, v as u32);
            }
        }
        cpu.mem.wr32(WIN_TARGET, 2);
        // The scene-ready bit state `0x37` waits on (`0x801D04A8`), which
        // the port takes as up.
        cpu.mem.wr32(0x1F80_0394, 0x80);
        // The developer menu stays shut: retail boots its gate to zero and
        // never sets it (`baka_cabinet::action_dump`), and its rows edit
        // words the port's menu does not model.
        let dev = false;
        cpu.mem.wr32(DEV_MENU, u32::from(dev));
        let mut cam = DuelCamera::default();
        let step = 1 + rng.below(3) as i32;
        cpu.mem.wr8(FRAME_STEP, step as u8);
        let (mut wins, mut decided) = ([0u32; 2], false);
        let mut duel_frames = 0;
        let mut drain_left = 0;
        for frame in 0..30000 {
            let state = port.state();
            seen.insert(state);
            // The pad: what moves each screen on, now and then, and noise.
            let mut edge: u16 = 0;
            if rng.below(6) == 0 {
                edge = [
                    0x0800, 0x0040, 0x0020, 0x8000, 0x2000, 0x1000, 0x4000, 0x0010,
                ][rng.below(8) as usize];
            }
            if state == ST_DUEL && edge & 0x0110 != 0 && rng.below(4) != 0 {
                edge = 0;
            }
            // Most runs take NEXT GAME, so the ladder runs to its end.
            if state == legaia_engine_minigames::baka_cabinet::ST_CHOICE
                && session % 3 != 0
                && edge & 0xA000 != 0
            {
                edge = 0;
            }
            let held = edge | (rng.next() as u16 & 0x00F0);
            cpu.mem.wr32(EDGE, u32::from(edge));
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr32(EDGE_ALT, 0);
            cpu.mem.wr16(CUE, 0);
            // The round: decided some frames in, by a coin.
            if state == ST_DUEL {
                duel_frames += 1;
                if !decided && duel_frames > 20 + rng.below(40) {
                    decided = true;
                    // The player takes most rounds, so runs climb the ladder
                    // to the secret rungs and the all-clear.
                    wins[usize::from(rng.below(6) == 0)] += 1;
                }
            } else if state == ST_ROUND_SETUP {
                duel_frames = 0;
                decided = false;
            }
            if state == legaia_engine_minigames::baka_cabinet::ST_PERFECT && drain_left == 0 {
                drain_left = 1 + rng.below(200);
            }
            let tally_draining = drain_left > 1;
            if drain_left > 1 {
                drain_left -= 1;
            }
            if state == legaia_engine_minigames::baka_cabinet::ST_TALLY_OUT {
                drain_left = 0;
            }
            draining.set(tally_draining);
            // The tally drain's running score, which the secret rungs' gate
            // reads: published into both, as the fight host does.
            if state == legaia_engine_minigames::baka_cabinet::ST_TALLY && rng.below(8) == 0 {
                let score = port.high_score() + rng.below(0x8000) as i32 * 8;
                port.set_high_score(score);
                cpu.mem.wr32(0x801D_BEE4, score as u32);
            }
            if wins.contains(&2) && state == ST_ROUND_SETUP {
                wins = [0, 0];
            }
            cpu.mem.wr32(PLAYER_WINS, wins[0]);
            cpu.mem.wr32(OPPONENT_WINS, wins[1]);
            // The duel camera beat, as the fight host runs it ahead of the
            // cabinet (`round_start_camera_beat`).
            let secret = port.secret_opponent();
            let first = secret != SecretOpponent::None && port.round() == 0;
            let camera_hold = match state {
                ST_ROUND_BANNER_A => {
                    if first {
                        cam.secret_intro(secret.raw());
                    }
                    false
                }
                ST_ROUND_BANNER_B => first && cam.secret_rise(step),
                ST_ROUND_BANNER_C => cam.spin(step),
                _ => false,
            };
            // The combat ticks' pause gate, up most frames.
            let pause_ready = rng.below(3) != 0;
            cpu.mem.wr32(PAUSE_GATE, u32::from(pause_ready));
            let roster = cpu.mem.rd32(ROSTER_ID);
            let rung_prize = cpu.mem.rd32(ROSTER + roster.min(0x10) * 0x6C + 0x20);
            let input = CabinetInput {
                frame_step: step,
                pad_edge: edge,
                pad_held: held,
                win_target: 2,
                player_round_wins: wins[0],
                opponent_round_wins: wins[1],
                rung_prize,
                round_clock: decided,
                camera_hold,
                pause_ready,
                tally_draining,
                dev_menu_enabled: dev,
                ..Default::default()
            };
            let f = port.tick(&input);
            // Every round setup snaps the camera and arms the spin
            // (`0x801CFF34..0x801CFF7C`), as the fight host's round reset does.
            if f.state == ST_ROUND_SETUP {
                cam.round_setup();
            }
            let timer_before = cpu.mem.rd32(ROUND_TIMER) as i32;
            assert_eq!(
                cpu.call(CABINET, &[ACTOR]),
                Stop::Returned,
                "session {session} frame {frame}"
            );
            // The round SM's clock, after the machine's frame.
            // (Not on the frame the pause menu opens: it zeroes the word.)
            if decided
                && state == ST_DUEL
                && timer_before < 0xB5
                && cpu.mem.rd32(0x801D_BF44) != 0xBF
            {
                let t = cpu.mem.rd32(ROUND_TIMER) as i32;
                cpu.mem.wr32(ROUND_TIMER, (t + step) as u32);
            }
            let at = format!("session {session} frame {frame} state {state:#x} edge {edge:#06x}");
            for (a, v) in port.retail_words() {
                assert_eq!(word(&mut cpu, a), v, "{a:#010x}, {at}");
            }
            let cue = cpu.mem.rd16(CUE);
            assert_eq!(cue, f.cues.last().map_or(0, |&c| u16::from(c)), "cue, {at}");
            frames_run += 1;
            if port.state() == ST_EXIT && port.exit_done() {
                break;
            }
        }
    }
    eprintln!("states seen {seen:x?}, frames {frames_run}");
    for want in [0x6D, 0xFA, 0xFE, 0x96] {
        assert!(
            seen.contains(&want),
            "state {want:#x} never reached: {seen:x?}"
        );
    }
}
