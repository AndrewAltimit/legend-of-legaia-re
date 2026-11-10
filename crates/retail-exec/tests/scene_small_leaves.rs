//! Small `SCUS_942.54` leaves of the scene, model and effect layers, each
//! against the Rust that carries its `PORT:` tag:
//!
//! - `FUN_800557B8`, the action-entry record copy, against the extent
//!   `parse_animation_stream` reads at `+0xAC`;
//! - `FUN_8003D0BC`, the record name-field skip, against `name_field_len`;
//! - `FUN_80050E74`, the part-actor pool flush, against
//!   `flush_part_actor_pool`;
//! - `FUN_800468A4`, the VRAM rect-copy enqueue with libgpu's `SetDrawMove`
//!   live beneath it, against `vram_rect_copy::enqueue`;
//! - `FUN_8005126C`, the battle on-screen test with the billboard projector
//!   live beneath it, against `battle_actor_on_screen`.

use crate::common::{Rng, scus_cpu};
use legaia_actor_formats::monster_archive::parse_animation_stream;
use legaia_engine_field::man_field_scripts::name_field_len;
use legaia_engine_vm::move_vm::{ActorState, PART_POOL_SLOTS, flush_part_actor_pool};
use legaia_engine_vm::vram_rect_copy::{EnqueueOutcome, RectCopyCall, enqueue};
use legaia_render_kernels::gte::{GteMat3, GteVec3};
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const RECORD_COPY: u32 = 0x8005_57B8;
const NAME_SKIP: u32 = 0x8003_D0BC;
const POOL_FLUSH: u32 = 0x8005_0E74;
const RECT_COPY: u32 = 0x8004_68A4;
const ON_SCREEN: u32 = 0x8005_126C;
const SRC: u32 = 0x8010_0000;
const DST: u32 = 0x8014_0000;
const STACK: u32 = 0x801F_FF00;

#[test]
fn the_record_copy_takes_what_the_stream_parser_reads() {
    differential!(scus(RECORD_COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5245_4343);
    let (mut parsed, mut empty) = (0, 0);
    for round in 0..1_500 {
        let (parts, frames) = match rng.below(8) {
            0 => (0, rng.below(4) as u8),
            1 => (rng.below(4) as u8, 0),
            _ => (1 + rng.below(12) as u8, 1 + rng.below(20) as u8),
        };
        let stream = usize::from(parts) * usize::from(frames) * 9;
        let mut rec: Vec<u8> = (0..0xAC + 2 + stream + 64).map(|_| rng.next() as u8).collect();
        rec[0xAC] = parts;
        rec[0xAD] = frames;
        cpu.mem.load(SRC, &rec);
        cpu.mem.load(DST, &vec![0xA5; rec.len()]);
        assert_eq!(cpu.call(RECORD_COPY, &[DST, SRC]), Stop::Returned);
        let copied = (cpu.regs.r[V0] - DST) as usize;
        // The header's `0x2B` words, then the stream's two count bytes and
        // nine bytes a part a frame, to the word.
        assert_eq!(copied, 0xAC + (2 + stream).next_multiple_of(4), "round {round}");
        let retail = cpu.mem.read(DST, copied);
        assert!(retail[..] == rec[..copied], "round {round}: copied bytes");
        assert!(
            cpu.mem.read(DST + copied as u32, 8) == [0xA5; 8],
            "round {round}: past the copy"
        );
        let whole = parse_animation_stream(&rec, 0, 0, 0, 0xAC, Vec::new());
        let from_copy = parse_animation_stream(&retail, 0, 0, 0, 0xAC, Vec::new());
        assert_eq!(whole, from_copy, "round {round}: the copy holds the stream");
        match whole {
            Some(a) => {
                assert_eq!((a.part_count, a.frame_count), (parts.into(), frames.into()));
                // One word short loses the stream's tail.
                assert!(
                    parse_animation_stream(&retail[..copied - 4], 0, 0, 0, 0xAC, Vec::new())
                        .is_none(),
                    "round {round}: the extent is tight"
                );
                parsed += 1;
            }
            None => empty += 1,
        }
    }
    assert!(parsed > 800 && empty > 200, "{parsed} {empty}");
}

#[test]
fn the_name_skip_is_the_name_field_length() {
    differential!(scus(NAME_SKIP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4E41_4D45);
    for n in 0..=255u32 {
        let mut body: Vec<u8> = (0..1 + 2 * n + 8).map(|_| rng.next() as u8).collect();
        body[0] = n as u8;
        cpu.mem.load(SRC, &body);
        assert_eq!(cpu.call(NAME_SKIP, &[SRC]), Stop::Returned);
        let port = name_field_len(&body).expect("a count byte");
        assert_eq!((cpu.regs.r[V0] - SRC) as usize, port, "count {n}");
    }
}

#[test]
fn the_pool_flush_retires_what_the_port_retires() {
    differential!(scus(POOL_FLUSH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const POOL: u32 = 0x801C_90F0;
    const ACTORS: u32 = 0x8010_0000;
    const ACTOR_LEN: u32 = 0x100;
    let mut rng = Rng(0x464C_5553);
    let mut retired_total = 0;
    for round in 0..300 {
        let mut before: Vec<Option<Vec<u8>>> = Vec::new();
        let mut actors: Vec<Option<ActorState>> = Vec::new();
        for slot in 0..PART_POOL_SLOTS as u32 {
            let at = ACTORS + ACTOR_LEN * slot;
            if rng.below(3) == 0 {
                cpu.mem.wr32(POOL + 4 * slot, 0);
                before.push(None);
                actors.push(None);
                continue;
            }
            let rec: Vec<u8> = (0..ACTOR_LEN).map(|_| rng.next() as u8).collect();
            cpu.mem.load(at, &rec);
            cpu.mem.wr32(POOL + 4 * slot, at);
            actors.push(Some(ActorState {
                flags: u32::from_le_bytes(rec[0x10..0x14].try_into().unwrap()),
                wait_timer: i16::from_le_bytes(rec[0x54..0x56].try_into().unwrap()),
                field_8c: u16::from_le_bytes(rec[0x8C..0x8E].try_into().unwrap()),
                ..ActorState::default()
            }));
            before.push(Some(rec));
        }
        // The words either side of the table stand.
        cpu.mem.wr32(POOL - 4, 0x1234_5678);
        cpu.mem.wr32(POOL + 4 * PART_POOL_SLOTS as u32, 0x8765_4321);
        assert_eq!(cpu.call(POOL_FLUSH, &[]), Stop::Returned, "round {round}");
        assert_eq!(cpu.mem.rd32(POOL - 4), 0x1234_5678);
        assert_eq!(cpu.mem.rd32(POOL + 4 * PART_POOL_SLOTS as u32), 0x8765_4321);

        let mut slots: Vec<Option<&mut ActorState>> =
            actors.iter_mut().map(Option::as_mut).collect();
        let retired = flush_part_actor_pool(&mut slots);
        assert!(slots.iter().all(Option::is_none), "round {round}");
        assert_eq!(retired, before.iter().flatten().count(), "round {round}");
        retired_total += retired;
        for (slot, (rec, actor)) in before.iter().zip(&actors).enumerate() {
            assert_eq!(cpu.mem.rd32(POOL + 4 * slot as u32), 0, "round {round}");
            let (Some(rec), Some(actor)) = (rec, actor) else {
                continue;
            };
            let mut want = rec.clone();
            want[0x10..0x14].copy_from_slice(&actor.flags.to_le_bytes());
            want[0x54..0x56].copy_from_slice(&actor.wait_timer.to_le_bytes());
            want[0x8C..0x8E].copy_from_slice(&actor.field_8c.to_le_bytes());
            let at = ACTORS + ACTOR_LEN * slot as u32;
            assert!(
                cpu.mem.read(at, ACTOR_LEN as usize) == want,
                "round {round}: slot {slot}"
            );
        }
    }
    assert!(retired_total > 20_000);
}

#[test]
fn the_rect_copy_enqueue_builds_the_port_packet() {
    differential!(scus(RECT_COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SCRATCH: u32 = 0x1F80_0314;
    const PACKETS: u32 = 0x8010_0000;
    const OT: u32 = 0x8018_0000;
    const BACK_BUFFER: u32 = 0x8007_B74C;
    const ADD_PRIM: u32 = 0x8003_D2C4;
    let linked: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    {
        let linked = linked.clone();
        cpu.hook(ADD_PRIM, move |r, _| {
            linked.borrow_mut().push((r.r[A0], r.r[A1]));
        });
    }
    let mut rng = Rng(0x5243_5059);
    let (mut built, mut refused, mut dead) = (0, 0, 0);
    for round in 0..4_000 {
        let ot_len = rng.below(0x40) as u16;
        let ot_slot = match rng.below(6) {
            0 => 0,
            1 => -(rng.below(4) as i32),
            2 => i32::from(ot_len) + rng.below(3) as i32 - 1,
            _ => rng.below(0x40) as i32,
        };
        let v = |rng: &mut Rng| match rng.below(6) {
            0 => 0i16,
            1 => rng.next() as i16,
            _ => rng.below(0x400) as i16,
        };
        let call = RectCopyCall {
            ot_slot,
            src_x: v(&mut rng),
            src_y: v(&mut rng),
            w: v(&mut rng),
            h: v(&mut rng),
            dst_x: v(&mut rng),
            dst_y: v(&mut rng),
        };
        let back = rng.below(2) == 1;
        cpu.mem.wr16(SCRATCH + 0x92, ot_len);
        cpu.mem.wr32(SCRATCH + 0x8C, PACKETS);
        cpu.mem.wr32(SCRATCH + 0xE0, OT);
        cpu.mem.wr16(BACK_BUFFER, u16::from(back) * (1 + rng.below(3) as u16));
        cpu.mem.load(PACKETS, &[0xA5; 0x20]);
        for (k, a) in [call.h, call.dst_x, call.dst_y].iter().enumerate() {
            cpu.mem.wr32(STACK + 0x10 + 4 * k as u32, *a as u16 as u32);
        }
        linked.borrow_mut().clear();
        let args = [
            ot_slot as u32,
            call.src_x as u16 as u32,
            call.src_y as u16 as u32,
            call.w as u16 as u32,
        ];
        assert_eq!(cpu.call(RECT_COPY, &args), Stop::Returned, "round {round}");
        let cursor = cpu.mem.rd32(SCRATCH + 0x8C);
        match enqueue(call, i32::from(ot_len), back) {
            EnqueueOutcome::SlotOutOfRange => {
                assert_eq!(cursor, PACKETS, "round {round}: nothing allocated");
                assert!(linked.borrow().is_empty(), "round {round}");
                assert!(cpu.mem.read(PACKETS, 0x20) == [0xA5; 0x20], "round {round}");
                refused += 1;
            }
            EnqueueOutcome::Linked { ot_slot, packet } => {
                assert_eq!(cursor, PACKETS + 0x18, "round {round}: one packet");
                assert_eq!(
                    *linked.borrow(),
                    [(OT + 4 * ot_slot as u32, PACKETS)],
                    "round {round}: link"
                );
                // The tag's address bytes are the linker's; the rest is the
                // builder's.
                assert_eq!(cpu.mem.rd8(PACKETS + 3), packet.tag_len, "round {round}");
                let w: [u32; 5] = std::array::from_fn(|k| cpu.mem.rd32(PACKETS + 4 + 4 * k as u32));
                assert_eq!(
                    w,
                    [
                        packet.word1,
                        packet.command,
                        packet.src,
                        packet.dst,
                        packet.extent
                    ],
                    "round {round}: {call:?} back {back}"
                );
                dead += usize::from(packet.is_skipped());
                built += 1;
            }
        }
    }
    assert!(
        built > 1_000 && refused > 1_000 && dead > 100,
        "{built} {refused} {dead}"
    );
}

#[test]
fn the_battle_on_screen_test_matches() {
    differential!(scus(ON_SCREEN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SAVED_CAMERA: u32 = 0x1F80_03C8;
    const OT_SHIFT: u32 = 0x1F80_03A4;
    const SEATS: u32 = 0x801C_9370;
    const SPRITE: u32 = 0x8010_0000;
    const SEAT_ACTORS: u32 = 0x8011_0000;
    let mut rng = Rng(0x4F4E_5343);
    let (mut on, mut off_left, mut off_right) = (0, 0, 0);
    for round in 0..6_000 {
        let a = |rng: &mut Rng| rng.below(0x1000) as u16;
        let rot = GteMat3::rot_x_psx(a(&mut rng) / 8).mul(&GteMat3::rot_y_psx(a(&mut rng)));
        let tr = [
            rng.below(0x1000) as i32 - 0x800,
            rng.below(0x400) as i32 - 0x200,
            rng.below(0x2000) as i32,
        ];
        let e: Vec<i16> = rot.m.iter().flatten().copied().collect();
        let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
        let words = [
            p(e[0], e[1]),
            p(e[2], e[3]),
            p(e[4], e[5]),
            p(e[6], e[7]),
            e[8] as u16 as u32,
            tr[0] as u32,
            tr[1] as u32,
            tr[2] as u32,
        ];
        for (k, w) in words.iter().enumerate() {
            cpu.gte.write_ctrl(k, *w);
            cpu.mem.wr32(SAVED_CAMERA + 4 * k as u32, *w);
        }
        let h = 0x100 + rng.below(0x200);
        let (ofx, ofy) = (160, 120);
        cpu.gte.write_ctrl(24, (ofx << 16) as u32);
        cpu.gte.write_ctrl(25, (ofy << 16) as u32);
        cpu.gte.write_ctrl(26, h);
        let shift = rng.below(4);
        cpu.mem.wr8(OT_SHIFT, shift as u8);

        // The sprite, its seat index and half size; the seat actor's position
        // at an odd address, as the unaligned copy allows.
        let seat = rng.below(8);
        let half = rng.below(0x80) as i16;
        let pos = [0, 1, 2].map(|_| rng.below(0x1000) as i16 - 0x800);
        let sprite: Vec<u8> = (0..0x80).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SPRITE, &sprite);
        cpu.mem.wr16(SPRITE + 0x5A, seat as u16);
        cpu.mem.wr16(SPRITE + 0x58, half as u16);
        let actor = SEAT_ACTORS + 0x200 * seat + (round & 3);
        for k in 0..8 {
            cpu.mem.wr32(SEATS + 4 * k, 0);
        }
        cpu.mem.wr32(SEATS + 4 * seat, actor);
        let pad = rng.next() as u16;
        for (k, v) in pos.iter().enumerate() {
            cpu.mem.wr16(actor + 0x3C + 2 * k as u32, *v as u16);
        }
        cpu.mem.wr16(actor + 0x42, pad);
        assert_eq!(cpu.call(ON_SCREEN, &[SPRITE]), Stop::Returned, "round {round}");
        let retail = cpu.regs.r[V0];

        let port = legaia_render_kernels::battle_on_screen::battle_actor_on_screen(
            &rot,
            GteVec3::new(tr[0], tr[1], tr[2]),
            (pos[0], pos[1], pos[2]),
            half,
            h as i32,
            ofx,
            ofy,
            shift,
        );
        assert_eq!(retail, u32::from(port.on_screen), "round {round}: {pos:?} half {half}");
        // The re-anchor: the seat's whole SVECTOR onto the sprite's `+0x14`,
        // and nothing else of it.
        let mut want = sprite.clone();
        want[0x58..0x5A].copy_from_slice(&half.to_le_bytes());
        want[0x5A..0x5C].copy_from_slice(&(seat as u16).to_le_bytes());
        let (x, y, z) = port.position;
        for (k, v) in [x as u16, y as u16, z as u16, pad].iter().enumerate() {
            want[0x14 + 2 * k..0x16 + 2 * k].copy_from_slice(&v.to_le_bytes());
        }
        assert!(cpu.mem.read(SPRITE, 0x80) == want, "round {round}: sprite");
        if port.on_screen {
            on += 1;
        } else if port.corners.xy[0].0 < 0 {
            off_left += 1;
        } else {
            off_right += 1;
        }
    }
    assert!(
        on > 500 && off_left > 200 && off_right > 200,
        "{on} {off_left} {off_right}"
    );
}
