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

/// The natural end pinned pass by pass at every per-pass advance the battle
/// plays (`2`, `3`, `4`, `8`, `16` sixteenths: rate `4` / `2` / `8` with clip
/// rates `1`, `2`, `4` and frame steps `1..3`) over every keyframe count up
/// to `40`: the commit is called on exactly the pass the advanced cursor
/// reaches `count << 4` (`slt` of `+0x68 >> 4` against the stream's byte `1`
/// at `0x80047A48..0x80047A60`, the rate added at `0x80047764` first), and on
/// that pass the walks read the advanced cursor - the frame count or past it -
/// which is the port's `crossing_frame`.
#[test]
fn the_natural_end_lands_on_the_count_at_every_advance() {
    differential!(scus(TICK));
    let Some(mut cpu) = retail() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // (actor rate, clip rate, frame step) -> advance in sixteenths.
    let shapes: [(u8, u8, u8, u16); 6] = [
        (4, 1, 1, 2),
        (2, 1, 3, 3),
        (8, 1, 1, 4),
        (8, 2, 1, 8),
        (8, 4, 1, 16),
        (8, 2, 2, 16),
    ];
    let mut failures = Vec::new();
    for &(r, clip_rate, dt, advance) in &shapes {
        for frames in 2..=40usize {
            cpu.mem.wr16(NODE + 0x5A, 0);
            cpu.mem.wr32(NODE + 0x4C, ENTRY);
            cpu.mem.wr16(NODE + 0x68, 0);
            cpu.mem.wr8(ENTRY + 0x78, clip_rate);
            cpu.mem.wr8(ENTRY + 0x85, 0);
            cpu.mem.wr8(ENTRY + 0x86, 0);
            cpu.mem.wr16(ENTRY + 0xE, 0);
            cpu.mem.wr8(ENTRY + 0x76, 0);
            cpu.mem.wr32(ENTRY + 0x88, STREAM);
            cpu.mem.wr8(STREAM, 1);
            cpu.mem.wr8(STREAM + 1, frames as u8);
            cpu.mem.wr8(ACTOR + 0x21D, r);
            cpu.mem.wr16(ACTOR + 0x176, 0);
            cpu.mem.wr8(ACTOR + 0x21B, 0);
            cpu.mem.wr16(ACTOR + 0x16E, 0);
            cpu.mem.wr8(ACTOR + 0x1D9, 0);
            cpu.mem.wr8(ACTOR + 0x1DC, 0);
            cpu.mem.wr32(ACTOR + 0x10, 0);
            cpu.mem.wr16(ACTOR + 0x178, 0);
            cpu.mem.wr8(FRAME_STEP, dt);
            let mut block = vec![1u8, frames as u8];
            block.resize(2 + frames * 9, 0);
            let anim = parse_animation_stream(&block, 0, clip_rate, 0, 0, vec![0u8; 0x88])
                .expect("stream");
            let mut p = MonsterAnimPlayer::new_one_shot(&anim).expect("player");
            let total = (frames as u16) << 4;
            let mut ended = false;
            for pass in 1..=(u32::from(total) / u32::from(advance) + 2) {
                cpu.calls.clear();
                assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned);
                // The commit is stubbed, so the node keeps the advanced cursor.
                let retail_cursor = cpu.mem.rd16(NODE + 0x68);
                let retail_end = cpu.calls.contains(&COMMIT);
                let _ = p.tick_rated_vsyncs(AnimRate(r), false, dt);
                let port_end = p.take_natural_end();
                let expect_end = u32::from(advance) * pass >= u32::from(total);
                let ctx = format!(
                    "advance {advance} (r {r} clip {clip_rate} dt {dt}) frames {frames} pass {pass}"
                );
                if retail_cursor as u32 != u32::from(advance) * pass {
                    failures.push(format!(
                        "{ctx}: retail cursor {retail_cursor} is not pass * advance"
                    ));
                    break;
                }
                if retail_end != expect_end || port_end != retail_end {
                    failures.push(format!(
                        "{ctx}: retail end {retail_end}, port end {port_end}, cursor {retail_cursor} of {total}"
                    ));
                    break;
                }
                if retail_end {
                    let walked = (retail_cursor >> 4) as i16;
                    if p.crossing_frame() != Some(walked) {
                        failures.push(format!(
                            "{ctx}: retail walks frame {walked}, port crossing {:?}",
                            p.crossing_frame()
                        ));
                    }
                    ended = true;
                    break;
                }
            }
            if !ended && failures.len() < 16 {
                failures.push(format!("advance {advance} frames {frames}: no natural end"));
            }
            if failures.len() >= 16 {
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tick's tail on an advancing seat (`0x80047F10..0x80048060`): a party
/// seat's Rage stamp (record `+0xF8 & 0x2000`, skipped under the battle-end
/// signal `DAT_8007BD71 == 0xFE` and while `+0x21C` is up) against
/// `World::stamp_rage_delegation`, and the history ring's id `+0x1FB` - the
/// committed `+0x1D9` on a party seat, the entry's `+0x87` / `+0x77` on a
/// monster's - against `battle_afterimage`; and the frozen-idle exit ahead of
/// it (`+0x16E & 0x404` with `+0x1D9 == 0`).
#[test]
fn the_tail_stamps_what_retail_stamps() {
    differential!(scus(TICK));
    use legaia_engine_battle::battle_afterimage::{monster_ring_id, party_ring_id};
    use legaia_engine_battle_vm::battle_action::BattleEndCause;
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_save::Party;
    const RECORDS: u32 = 0x8008_4708;
    const SIGNAL: u32 = 0x8007_BD71;
    let Some(mut cpu) = retail() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 2);
    w.party.party_count = 3;
    let mut rng = Rng(0x5241_4745);
    let (mut stamped, mut monsters, mut frozen) = (0u32, 0u32, 0u32);
    for round in 0..20_000 {
        let seat = rng.below(7);
        let actor = ACTOR + seat * 0x400;
        cpu.mem.wr32(ACTORS + seat * 4, actor);
        let char_id = if seat < 3 { 1 + rng.below(4) as u8 } else { 4 };
        cpu.mem.wr8(SEATS + seat, char_id);
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x74, 0x10);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        cpu.mem.wr16(NODE + 0x68, 0);
        cpu.mem.wr8(ENTRY + 0x78, 1);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        cpu.mem.wr8(STREAM + 1, 9);
        let (flag_87, key_77) = (rng.below(3) as u8, rng.below(0x40) as u8);
        cpu.mem.wr8(ENTRY + 0x87, flag_87);
        cpu.mem.wr8(ENTRY + 0x77, key_77);
        cpu.mem.wr8(actor + 0x21D, 8);
        cpu.mem.wr32(actor + 0x10, 0);
        cpu.mem.wr16(actor + 0x178, 0);
        cpu.mem.wr8(actor + 0x1DC, 0);
        cpu.mem.wr8(FRAME_STEP, 1);
        let committed = if rng.below(4) == 0 {
            0
        } else {
            1 + rng.below(0x20) as u8
        };
        let status = [
            0u16, 0x0001, 0x0080, 0x0200, 0x0004, 0x0400, 0x0404, 0x1000, 0x1001,
        ][rng.below(9) as usize];
        let (render, impact) = (
            (rng.below(4) == 0) as u8,
            [0u8, 0, 1, 3][rng.below(4) as usize],
        );
        let colour = rng.next();
        let ability = if rng.below(2) == 0 { 0x2000 } else { 0 } | (rng.next() & !0x2000);
        let signal = [0u8, 0, 0xFE, 0x10][rng.below(4) as usize];
        cpu.mem.wr8(actor + 0x1D9, committed);
        cpu.mem.wr16(actor + 0x16E, status);
        cpu.mem.wr8(actor + 0x21C, render);
        cpu.mem.wr8(actor + 0x21F, impact);
        cpu.mem.wr32(actor + 0x04, colour);
        cpu.mem.wr32(actor + 0x0C, 0);
        cpu.mem.wr8(SIGNAL, signal);
        if seat < 3 {
            cpu.mem
                .wr32(RECORDS + (u32::from(char_id) - 1) * 0x414 + 0xF8, ability);
        }
        cpu.calls.clear();
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round}");
        // The frozen idle leaves through the pose copy `FUN_8004A908`.
        let retail_frozen = cpu.calls.contains(&0x8004_A908);
        {
            let b = &mut w.actors[seat as usize];
            b.battle.field_flags = status;
            b.battle.current_anim = committed;
            b.battle_reaction = None;
        }
        w.battle.status_effects = Default::default();
        assert_eq!(
            retail_frozen,
            w.frozen_idle(seat as usize),
            "round {round} seat {seat} status {status:#x} committed {committed:#x}"
        );
        if retail_frozen {
            frozen += 1;
            continue;
        }
        // A Slowed idle advances on `>> 2`: rate 8, clip rate 1, step 1 moves
        // the cursor `2` sixteenths, the `>> 1` branch `4`.
        let retail_slowed = cpu.mem.rd16(NODE + 0x68) == 2;
        assert_eq!(
            retail_slowed,
            w.slowed_idle(seat as usize),
            "round {round} seat {seat} status {status:#x} committed {committed:#x}"
        );
        let retail = (
            cpu.mem.rd32(actor + 0x04),
            cpu.mem.rd32(actor + 0x0C),
            cpu.mem.rd16(actor + 0x16E),
            cpu.mem.rd8(actor + 0x21F),
            cpu.mem.rd8(actor + 0x1FB),
        );
        let port = if seat < 3 {
            for m in w.party.roster.members.iter_mut() {
                let mut bits = m.ability_bits();
                bits[4..8].copy_from_slice(&ability.to_le_bytes());
                m.set_ability_bits(bits);
            }
            w.battle.end = (signal == 0xFE).then_some(BattleEndCause::Escaped);
            let s = seat as usize;
            let b = &mut w.actors[s].battle;
            b.render_flag = render;
            b.impact_state = impact;
            b.field_flags = status;
            b.render_color = colour;
            b.render_blend = 0;
            w.rage_delegation(s);
            let b = &w.actors[s].battle;
            stamped += u32::from(b.field_flags != status);
            (
                b.render_color,
                b.render_blend,
                b.field_flags,
                b.impact_state,
                party_ring_id(committed),
            )
        } else {
            monsters += 1;
            (colour, 0, status, impact, monster_ring_id(key_77, flag_87))
        };
        assert_eq!(
            retail, port,
            "round {round} seat {seat} char {char_id} ability {ability:#x} signal {signal:#x} \
             render {render} 87 {flag_87} 77 {key_77:#x}"
        );
    }
    assert!(
        stamped > 1000 && monsters > 5000 && frozen > 500,
        "{stamped} {monsters} {frozen}"
    );
}

/// The zero-colour skip at the tick's head (`0x80047484..0x800474E0`) against
/// `World::battle_node_tick_skipped`, and the loop window's re-zero of the
/// hit index and effect / cue cursors (`0x80047840..0x80047878`) against
/// `World::loop_rewind_rezeroes`.
#[test]
fn the_skip_and_the_rezero_are_retails() {
    differential!(scus(TICK));
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_save::Party;
    let Some(mut cpu) = retail() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    w.load_party(Party::zeroed(3));
    w.enter_battle(3, 4);
    w.party.party_count = 3;
    let mut rng = Rng(0x5A45_524F);
    let (mut skipped, mut rezeroed) = (0u32, 0u32);
    for round in 0..20_000 {
        let seat = rng.below(7);
        let actor = ACTOR + seat * 0x400;
        cpu.mem.wr32(ACTORS + seat * 4, actor);
        cpu.mem
            .wr8(SEATS + seat, if seat < 3 { 1 + seat as u8 } else { 4 });
        let colour = [0u32, 0x10, 0x0100_0000, 0x80_8080][rng.below(4) as usize];
        cpu.mem.wr16(NODE + 0x5A, seat as u16);
        cpu.mem.wr32(NODE + 0x74, colour);
        cpu.mem.wr32(NODE + 0x4C, ENTRY);
        // A window `start < end` the advance crosses.
        let end = 4 + rng.below(8) as u8;
        let start = rng.below(u32::from(end)) as u8;
        let count = 1 + rng.below(3) as u16;
        cpu.mem.wr16(NODE + 0x68, (u16::from(end) << 4) - 2);
        cpu.mem.wr8(ENTRY + 0x78, 1);
        cpu.mem.wr8(ENTRY + 0x85, start);
        cpu.mem.wr8(ENTRY + 0x86, end);
        cpu.mem.wr16(ENTRY + 0xE, 0);
        cpu.mem.wr16(ENTRY + 0xC, 0);
        cpu.mem.wr8(ENTRY + 0x76, 0);
        cpu.mem.wr32(ENTRY + 0x88, STREAM);
        cpu.mem.wr8(STREAM, 1);
        cpu.mem.wr8(STREAM + 1, 0x30);
        cpu.mem.wr8(FRAME_STEP, 1);
        cpu.mem.wr8(actor + 0x21D, 8);
        cpu.mem.wr16(actor + 0x176, count << 4);
        cpu.mem.wr8(actor + 0x21B, count as u8);
        cpu.mem.wr16(actor + 0x16E, 0);
        let committed = [0x11u8, 0x11, 0x10, 0x0C][rng.below(4) as usize];
        let latched = 0x28 + rng.below(8) as u8;
        cpu.mem.wr8(actor + 0x1D9, committed);
        cpu.mem.wr8(actor + 0x1DB, latched);
        cpu.mem.wr8(actor + 0x1DC, 0);
        for k in 0x1F4..0x1F7 {
            cpu.mem.wr8(actor + k, 5);
        }
        cpu.mem.wr32(actor + 0x10, 7);
        cpu.mem.wr16(actor + 0x178, 0);
        assert_eq!(cpu.call(TICK, &[NODE]), Stop::Returned, "{round}");
        // A skipped tick leaves the readout word alone; any other ramps it.
        let retail_skipped = cpu.mem.rd32(actor + 0x10) == 7;
        let i = seat as usize;
        {
            let a = &mut w.actors[i];
            a.battle_monster_id = (seat >= 3).then_some(0x40);
            a.battle_node_colour_zero = Some(colour & 0x00FF_FFFF == 0);
            a.battle.current_anim = committed;
            a.battle.latched_anim = latched;
        }
        assert_eq!(
            retail_skipped,
            w.node_tick_skipped(i),
            "round {round} seat {seat} colour {colour:#x}"
        );
        if retail_skipped {
            skipped += 1;
            continue;
        }
        let retail_rezero = (0x1F4..0x1F7).all(|k| cpu.mem.rd8(actor + k) == 0);
        assert_eq!(
            retail_rezero,
            w.loop_rewind_rezeroes(i),
            "round {round} seat {seat} committed {committed:#x} latched {latched:#x}"
        );
        rezeroed += u32::from(retail_rezero);
    }
    assert!(skipped > 1000 && rezeroed > 1000, "{skipped} {rezeroed}");
}
