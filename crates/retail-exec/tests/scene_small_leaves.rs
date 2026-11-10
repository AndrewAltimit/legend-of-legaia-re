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

#[test]
fn the_initmap_override_is_sanitised_as_the_port_sanitises_it() {
    differential!(scus(BOOT_INIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_system::scene_name_sync::{
        INITMAP_NAME_FIELD_LEN, sanitize_initmap_scene_name,
    };
    const BOOT_INIT: u32 = 0x8001_D424;
    const BOOT_INIT_END: u32 = 0x8001_D7F8;
    const FILE_LOAD: u32 = 0x8003_E6BC;
    const NAME_FIELD: u32 = 0x8007_050C;
    /// `_DAT_8007B8C2`, clear on the development host that reads the file.
    const BY_INDEX: u32 = 0x8007_B8C2;
    /// `_DAT_8007B86E`, raised by a load that found no file.
    const LOAD_FAILED: u32 = 0x8007_B86E;
    // Everything the routine calls is display, GTE and work-table setup;
    // only the file load and the field copy matter to the name.
    cpu.stub_outside = Some((BOOT_INIT, BOOT_INIT_END));
    let line: Rc<RefCell<Vec<u8>>> = Rc::default();
    {
        let line = line.clone();
        cpu.hook(FILE_LOAD, move |r, m| {
            m.load(r.r[A1], &line.borrow());
            r.r[V0] = 0;
        });
    }
    cpu.hook(0x8001_A8B0, |r, m| {
        let bytes = m.read(r.r[A1], r.r[psx_retail_exec::A2] as usize);
        m.load(r.r[A0], &bytes);
    });
    // The allocator's blocks, somewhere harmless.
    cpu.stub(0x8001_7888, 0x8018_0000);
    let mut rng = Rng(0x494E_4954);
    let (mut cleaned, mut kept) = (0, 0);
    for round in 0..600 {
        let text: Vec<u8> = (0..INITMAP_NAME_FIELD_LEN + 8)
            .map(|_| match rng.below(8) {
                0 => 0x0D,
                1 => 0x0A,
                2 => 0x1A,
                3 => 0,
                _ => b'a' + rng.below(26) as u8,
            })
            .collect();
        *line.borrow_mut() = text.clone();
        let by_index = round % 5 == 4;
        cpu.mem.wr16(BY_INDEX, u16::from(by_index));
        cpu.mem.wr16(LOAD_FAILED, 0);
        cpu.mem.load(NAME_FIELD, b"default\0\0\0\0\0\0\0\0\0");
        assert_eq!(cpu.call(BOOT_INIT, &[]), Stop::Returned, "round {round}");
        let retail = cpu.mem.read(NAME_FIELD, INITMAP_NAME_FIELD_LEN);
        if by_index {
            // The disc build never reads the override.
            assert!(retail == b"default\0\0\0\0\0\0\0\0\0", "round {round}");
            kept += 1;
            continue;
        }
        let mut port = text[..INITMAP_NAME_FIELD_LEN].to_vec();
        sanitize_initmap_scene_name(&mut port);
        assert!(retail == port, "round {round}: {retail:02x?} / {port:02x?}");
        cleaned += usize::from(port != text[..INITMAP_NAME_FIELD_LEN]);
    }
    assert!(cleaned > 300 && kept > 100, "{cleaned} {kept}");
}

#[test]
fn the_move_image_wrapper_queues_the_copy_the_port_makes() {
    differential!(scus(MOVE_IMAGE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use psx_tim::vram::{VRAM_HEIGHT, VRAM_WIDTH, Vram};
    const MOVE_IMAGE: u32 = 0x8005_8490;
    /// libgpu's driver table pointer, and the queue entry the wrapper calls
    /// through its `+0x08`.
    const DRIVER_PTR: u32 = 0x8007_8D4C;
    const DRIVER: u32 = 0x8010_1000;
    const QUEUE: u32 = 0x8010_2000;
    const RECT: u32 = 0x8010_0000;
    cpu.stub(0x8005_8170, 0); // the debug-level rect print
    cpu.mem.wr32(DRIVER_PTR, DRIVER);
    cpu.mem.wr32(DRIVER + 8, QUEUE);
    cpu.mem.wr32(DRIVER + 0x18, 0x8010_3000);
    // The GPU's queue: the command's three words.
    let queued: Rc<RefCell<Vec<[u32; 3]>>> = Rc::default();
    {
        let queued = queued.clone();
        cpu.hook(QUEUE, move |r, m| {
            assert_eq!(r.r[A0], 0x8010_3000, "the driver's copy routine");
            assert_eq!(r.r[psx_retail_exec::A2], 0x14, "five words");
            let p = r.r[A1];
            queued
                .borrow_mut()
                .push([m.rd32(p + 8), m.rd32(p + 0xC), m.rd32(p + 0x10)]);
            r.r[V0] = 0;
        });
    }
    let mut rng = Rng(0x4D4F_5645);
    let seed: Vec<u8> = (0..VRAM_WIDTH * VRAM_HEIGHT * 2)
        .map(|_| rng.next() as u8)
        .collect();
    let mut port = Vram::new();
    port.write_block(0, 0, VRAM_WIDTH as u16, VRAM_HEIGHT as u16, &seed);
    let mut gpu: Vec<u16> = port.as_u16().to_vec();
    let (mut copies, mut refused) = (0, 0);
    for round in 0..400 {
        let (w, h) = match rng.below(6) {
            0 => (0, 1 + rng.below(64) as u16),
            1 => (1 + rng.below(64) as u16, 0),
            _ => (1 + rng.below(64) as u16, 1 + rng.below(64) as u16),
        };
        // The source in the left half and the destination in the right, so
        // the stand-in below needs no overlap rule.
        let (sx, sy) = (rng.below(440) as u16, rng.below(440) as u16);
        let (dx, dy) = (512 + rng.below(440) as u16, rng.below(440) as u16);
        for (k, v) in [sx, sy, w, h].iter().enumerate() {
            cpu.mem.wr16(RECT + 2 * k as u32, *v);
        }
        queued.borrow_mut().clear();
        let args = [RECT, u32::from(dx), u32::from(dy)];
        assert_eq!(cpu.call(MOVE_IMAGE, &args), Stop::Returned, "round {round}");
        let q = queued.borrow().clone();
        if w == 0 || h == 0 {
            assert!(q.is_empty(), "round {round}: an empty rect queues nothing");
            assert_eq!(cpu.regs.r[V0], -1i32 as u32, "round {round}");
            refused += 1;
        } else {
            assert_eq!(q.len(), 1, "round {round}");
            // Source corner, destination corner, extent: `y << 16 | x`.
            let [src, dst, ext] = q[0];
            let (x0, y0) = ((src & 0xFFFF) as usize, (src >> 16) as usize);
            let (x1, y1) = ((dst & 0xFFFF) as usize, (dst >> 16) as usize);
            let (cw, ch) = ((ext & 0xFFFF) as usize, (ext >> 16) as usize);
            for row in 0..ch {
                for col in 0..cw {
                    gpu[(y1 + row) * VRAM_WIDTH + x1 + col] =
                        gpu[(y0 + row) * VRAM_WIDTH + x0 + col];
                }
            }
            copies += 1;
        }
        port.move_image(sx, sy, w, h, dx, dy);
        assert!(port.as_u16() == &gpu[..], "round {round}: VRAM after the copy");
    }
    assert!(copies > 200 && refused > 80, "{copies} {refused}");
}

#[test]
fn the_list_walk_releases_the_emitters_the_port_releases() {
    differential!(scus(LIST_WALK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_vm::sfx_emitter::{EmitterOp, emitter_free};
    const LIST_WALK: u32 = 0x8002_519C;
    const PART_TICK: u32 = 0x8002_1DF4;
    const OTHER_TICK: u32 = 0x8010_F000;
    const RELEASE: u32 = 0x8002_50D4;
    const HEAD: u32 = 0x8010_0000;
    const NODES: u32 = 0x8010_1000;
    const NODE_LEN: u32 = 0x100;
    const HALT: u32 = 0x8;
    const MARKED: u32 = 0x0200_0000;
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Call {
        Tick(u32, u32),
        Unlink(u32),
        FreeNode(u32),
        Free(u32),
        Release(i32, i16),
    }
    let log: Rc<RefCell<Vec<Call>>> = Rc::default();
    macro_rules! hook {
        ($va:expr, $f:expr) => {{
            let log = log.clone();
            cpu.hook($va, move |r, _| {
                let f: fn(&psx_retail_exec::Regs) -> Call = $f;
                log.borrow_mut().push(f(r));
            });
        }};
    }
    hook!(PART_TICK, |r| Call::Tick(PART_TICK, r.r[A0]));
    hook!(OTHER_TICK, |r| Call::Tick(OTHER_TICK, r.r[A0]));
    hook!(0x8002_4DFC, |r| Call::Unlink(r.r[A0]));
    hook!(0x8002_04A4, |r| Call::FreeNode(r.r[A0]));
    hook!(0x8001_7B94, |r| Call::Free(r.r[A0]));
    hook!(RELEASE, |r| Call::Release(r.r[A0] as i32, r.r[A1] as i16));
    let mut rng = Rng(0x4C49_5354);
    let (mut released, mut freed_quiet, mut ticked, mut marked_now) = (0, 0, 0, 0);
    for round in 0..1_500 {
        let n = rng.below(10);
        let mut nodes = Vec::new();
        for k in 0..n {
            let at = NODES + NODE_LEN * k;
            let mut rec: Vec<u8> = (0..NODE_LEN).map(|_| rng.next() as u8).collect();
            let next = if k + 1 < n { at + NODE_LEN } else { 0 };
            rec[0..4].copy_from_slice(&next.to_le_bytes());
            let tick = if rng.below(3) == 0 { OTHER_TICK } else { PART_TICK };
            rec[0xC..0x10].copy_from_slice(&tick.to_le_bytes());
            let flags = (rng.next() & !(HALT | MARKED | 0x800))
                | [0, HALT, HALT | MARKED, HALT | MARKED][rng.below(4) as usize];
            rec[0x10..0x14].copy_from_slice(&flags.to_le_bytes());
            let mode = [3i16, 5, 5, 1, 7][rng.below(5) as usize];
            rec[0x5A..0x5C].copy_from_slice(&mode.to_le_bytes());
            // The mode-3 block and the `+0x4C` buffer, absent as often as not.
            for off in [0x4C, 0xA8] {
                if rng.below(2) == 0 {
                    rec[off..off + 4].copy_from_slice(&0u32.to_le_bytes());
                }
            }
            cpu.mem.load(at, &rec);
            nodes.push((at, rec, tick, flags, mode));
        }
        cpu.mem.wr32(HEAD, if n > 0 { NODES } else { 0 });
        log.borrow_mut().clear();
        assert_eq!(cpu.call(LIST_WALK, &[HEAD]), Stop::Returned, "round {round}");
        let calls = log.borrow().clone();
        let mut at_call = 0;
        for (at, rec, tick, flags, mode) in &nodes {
            let mine: Vec<Call> = {
                let start = at_call;
                while at_call < calls.len()
                    && match calls[at_call] {
                        Call::Tick(_, a) | Call::Unlink(a) | Call::FreeNode(a) => a == *at,
                        // The frees and the release name the node's buffers.
                        _ => at_call > start,
                    }
                {
                    at_call += 1;
                }
                calls[start..at_call].to_vec()
            };
            if flags & HALT == 0 {
                assert_eq!(mine, [Call::Tick(*tick, *at)], "round {round}: a live node ticks");
                ticked += 1;
                continue;
            }
            // Halted: the mark is raised either way.
            assert_ne!(cpu.mem.rd32(at + 0x10) & MARKED, 0, "round {round}");
            if flags & MARKED == 0 {
                assert!(mine.is_empty(), "round {round}: the first halted walk frees nothing");
                marked_now += 1;
                continue;
            }
            assert_eq!(mine[..2], [Call::Unlink(*at), Call::FreeNode(*at)], "round {round}");
            let release: Vec<Call> = mine
                .iter()
                .copied()
                .filter(|c| matches!(c, Call::Release(..)))
                .collect();
            // The port's half: the part tick's emitter mode, read off the node.
            let mut st = ActorState {
                move_submode: *mode,
                ..ActorState::default()
            };
            st.set_actor_u32(0xAC, u32::from_le_bytes(rec[0xAC..0xB0].try_into().unwrap()));
            st.set_actor_u16(0xB0, u16::from_le_bytes(rec[0xB0..0xB2].try_into().unwrap()));
            let port = (*tick == PART_TICK)
                .then(|| emitter_free(&st))
                .flatten()
                .map(|op| match op {
                    EmitterOp::Release { cue, voice } => Call::Release(cue, voice),
                    other => panic!("round {round}: {other:?}"),
                });
            assert_eq!(release, port.into_iter().collect::<Vec<_>>(), "round {round}");
            if release.is_empty() {
                freed_quiet += 1;
            } else {
                assert_eq!(mine.last(), release.last(), "round {round}: the release is last");
                released += 1;
            }
        }
        assert_eq!(at_call, calls.len(), "round {round}: calls for no node");
    }
    assert!(
        released > 300 && freed_quiet > 300 && ticked > 1_000 && marked_now > 1_000,
        "{released} {freed_quiet} {ticked} {marked_now}"
    );
}

#[test]
fn the_battle_loader_asks_for_the_stage_overlay_the_port_names() {
    differential!(scus(BATTLE_LOADER));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_scene::overlay_loader::{OVERLAY_PROT_BASE, battle_stage_overlay_entry};
    const BATTLE_LOADER: u32 = 0x8005_20F0;
    const BATTLE_LOADER_END: u32 = 0x8005_2770;
    /// The loader's state byte, `gp + 0xA59`.
    const STATE: u32 = 0x8007_BD71;
    const STAGE_ID: u32 = 0x8007_B64A;
    const LOADER_B: u32 = 0x8003_EC70;
    cpu.stub_outside = Some((BATTLE_LOADER, BATTLE_LOADER_END));
    let asked: Rc<RefCell<Vec<i32>>> = Rc::default();
    {
        let asked = asked.clone();
        cpu.hook(LOADER_B, move |r, _| asked.borrow_mut().push(r.r[A0] as i32));
    }
    let (mut loaded, mut none) = (0, 0);
    // Both states that reach the stage arm (`0x8005266C` / `0x80052670`).
    for state in [0xEu8, 0x10] {
        for stage in 0..=255u8 {
            cpu.mem.wr8(STATE, state);
            cpu.mem.wr8(STAGE_ID, stage);
            asked.borrow_mut().clear();
            assert_eq!(cpu.call(BATTLE_LOADER, &[]), Stop::Returned, "stage {stage}");
            let retail: Vec<u32> = asked
                .borrow()
                .iter()
                .map(|p| (p + OVERLAY_PROT_BASE) as u32)
                .collect();
            let port = battle_stage_overlay_entry(stage);
            assert_eq!(retail, port.into_iter().collect::<Vec<_>>(), "stage {stage}");
            // With no stage overlay the loader goes straight to its stream
            // tail (`0xFF`); with one it waits on the read first.
            assert_eq!(
                cpu.mem.rd8(STATE),
                if port.is_some() { 0x11 } else { 0xFF },
                "stage {stage}: next state"
            );
            if port.is_some() {
                loaded += 1;
            } else {
                none += 1;
            }
        }
    }
    assert_eq!((loaded, none), (510, 2));
}
