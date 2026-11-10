//! The per-frame anim-node cursor advance of `FUN_80047430`, its loop-window
//! arm and natural end, against `MonsterAnimPlayer::tick_rated_vsyncs`: every
//! rate, frame step, Slowed-idle gate, window shape and clip length, tick by
//! tick. Calls outside the routine are stubbed; a `jal` to the commit
//! `FUN_8004AD80` is the natural end.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::monster_archive::parse_animation_stream;
use legaia_engine_battle::battle_anim::MonsterAnimPlayer;
use legaia_engine_battle_vm::battle_anim_rate::AnimRate;
use psx_retail_exec::{Cpu, Stop};

const TICK: u32 = 0x8004_7430;
const COMMIT: u32 = 0x8004_AD80;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8014_0000;
const ENTRY: u32 = 0x8014_1000;
const STREAM: u32 = 0x8015_0000;
const FRAME_STEP: u32 = 0x1F80_0393;

fn retail() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.stub_outside = Some((TICK, 0x8004_8108));
    cpu.step_budget = 200_000;
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr8(SEATS, 1);
    Some(cpu)
}

#[test]
fn the_anim_cursor_is_retails() {
    differential!(scus(TICK));
    let Some(mut cpu) = retail() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x414E_494D);
    let (mut windowed, mut ended, mut failures) = (0u32, 0u32, Vec::new());
    for round in 0..15_000 {
        let frames = 2 + rng.below(30) as usize;
        let clip_rate = [1u8, 1, 2, 2, 3, 4][rng.below(6) as usize];
        let (count, start, end) = if rng.below(2) == 0 {
            let end = 1 + rng.below(frames as u32) as u8;
            (
                1 + rng.below(4) as u8,
                if rng.below(3) == 0 {
                    end
                } else {
                    rng.below(u32::from(end)) as u8
                },
                end,
            )
        } else {
            (0, 0, 0)
        };
        let r = [8u8, 8, 8, 4, 2, 0][rng.below(6) as usize];
        let dt = 1 + rng.below(3) as u8;
        let slowed = rng.below(5) == 0;
        windowed += u32::from(count != 0);

        cpu.mem.wr16(NODE + 0x5A, 0);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr16(NODE + 0x68, 0);
        cpu.mem.wr8(ENTRY + 0x78, clip_rate);
        cpu.mem.wr8(ENTRY + 0x85, start);
        cpu.mem.wr8(ENTRY + 0x86, end);
        cpu.mem.wr16(ENTRY + 0xE, 0);
        cpu.mem.wr8(ENTRY + 0x76, 0);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        cpu.mem.wr8(STREAM, 1);
        cpu.mem.wr8(STREAM + 1, frames as u8);
        cpu.mem.wr8(ACTOR + 0x21D, r);
        cpu.mem.wr16(ACTOR + 0x176, u16::from(count) << 4);
        cpu.mem.wr8(ACTOR + 0x21B, count);
        cpu.mem.wr16(ACTOR + 0x16E, if slowed { 0x1000 } else { 0 });
        cpu.mem.wr8(ACTOR + 0x1D9, 0);
        cpu.mem.wr8(ACTOR + 0x1DC, 0);
        cpu.mem.wr32(ACTOR + 0x10, 0);
        cpu.mem.wr16(ACTOR + 0x178, 0);
        cpu.mem.wr8(FRAME_STEP, dt);

        let mut head = vec![0u8; 0x88];
        head[0x84] = count;
        head[0x85] = start;
        head[0x86] = end;
        head[0x87] = 0;
        let mut block = vec![1u8, frames as u8];
        block.resize(2 + frames * 9, 0);
        let anim = parse_animation_stream(&block, 0, clip_rate, 0, 0, head).expect("stream");
        let mut p = MonsterAnimPlayer::new_one_shot(&anim).expect("player");

        for tick in 0..400 {
            cpu.calls.clear();
            assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round} {tick}");
            let retail_cursor = cpu.mem.rd16(NODE + 0x68);
            let retail_end = cpu.calls.contains(&COMMIT);

            let _ = p.tick_rated_vsyncs(AnimRate(r), slowed, dt);
            let port_end = p.take_natural_end();
            let port_cursor = p.cursor_sixteenths();
            let same = retail_end == port_end && (retail_end || retail_cursor == port_cursor);
            if !same && failures.len() < 16 {
                failures.push(format!(
                    "round {round} tick {tick} frames {frames} clip {clip_rate} r {r} dt {dt} slowed {slowed} \
                     window ({count}, {start}, {end}): retail {retail_cursor:#x} end {retail_end}, \
                     port {port_cursor:#x} end {port_end}"
                ));
                break;
            }
            if retail_end {
                ended += 1;
                break;
            }
            if r == 0 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(windowed > 3000 && ended > 5000, "{windowed} {ended}");
}

/// The same pass's HP and MP readout ramps (`0x800474E8..0x80047634`): a
/// quarter of the signed accumulator a frame on a party seat, the whole of it
/// on a monster's, against `battle_hp_bar`.
#[test]
fn the_readout_ramps_are_retails() {
    differential!(scus(TICK));
    use legaia_engine_battle_vm::battle_hp_bar::{
        monster_bar_step, monster_mp_step, party_bar_step, party_mp_step,
    };
    let Some(mut cpu) = retail() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4845_4C50);
    let mut failures = Vec::new();
    for round in 0..80_000 {
        let seat = rng.below(7);
        let actor = ACTOR + seat * 0x400;
        cpu.mem.wr32(ACTORS + seat * 4, actor);
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x74, 0x10);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr16(NODE + 0x68, 0);
        cpu.mem.wr8(ENTRY + 0x78, 1);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        cpu.mem.wr8(STREAM + 1, 9);
        cpu.mem.wr8(actor + 0x21D, 8);
        cpu.mem.wr16(actor + 0x16E, 0);
        cpu.mem.wr8(actor + 0x1D9, 1);
        cpu.mem.wr8(actor + 0x1DC, 0);
        cpu.mem.wr8(FRAME_STEP, 1);
        let hp_pending = match rng.below(4) {
            0 => rng.below(9) as i32 - 4,
            _ => rng.below(0x2000) as i32 - 0x1000,
        };
        let mp_pending = match rng.below(4) {
            0 => rng.below(9) as i16 - 4,
            _ => rng.below(0x800) as i16 - 0x400,
        };
        let (hp_shown, mp_shown) = (rng.next() as u16, rng.next() as u16);
        cpu.mem.wr32(actor + 0x10, hp_pending as u32);
        cpu.mem.wr16(actor + 0x172, hp_shown);
        cpu.mem.wr16(actor + 0x174, mp_shown);
        cpu.mem.wr16(actor + 0x178, mp_pending as u16);
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round}");
        let party = seat < 3;
        let hp = if party {
            party_bar_step(hp_shown, hp_pending)
        } else {
            monster_bar_step(hp_shown, hp_pending)
        };
        let (mp_d, mp_p) = if party {
            let s = party_mp_step(mp_shown, mp_pending);
            (s.display, s.pending as i16)
        } else {
            let s = monster_mp_step(mp_shown, mp_pending);
            (s.display, s.pending as i16)
        };
        // The monster arm of the `+0x178` branch runs the HP fields again
        // when the readout word is non-zero (`0x80047624..0x80047634`); the
        // accumulator is zero by then, so it subtracts nothing.
        let want = (hp.display, hp.pending, mp_d, mp_p);
        let got = (
            cpu.mem.rd16(actor + 0x172),
            cpu.mem.rd32(actor + 0x10) as i32,
            cpu.mem.rd16(actor + 0x174),
            cpu.mem.rd16(actor + 0x178) as i16,
        );
        if want != got && failures.len() < 12 {
            failures.push(format!(
                "round {round} seat {seat} hp ({hp_shown:#x}, {hp_pending:#x}) mp ({mp_shown:#x}, {mp_pending:#x}): \
                 retail {got:x?} port {want:x?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
