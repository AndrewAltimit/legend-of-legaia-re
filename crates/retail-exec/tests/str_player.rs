//! The STR overlay's play-loop pieces off the disc (PROT 0970, base
//! `0x801CE818`), against `psx_mdec::str_player`:
//!
//! - the decode-context init `FUN_801CF8B0`, the frame poll `FUN_801CF740`
//!   (its `StGetNext` answered with generated sector headers) and the
//!   MDEC-out slice callback `FUN_801CF56C` (its DMA kick and `LoadImage`
//!   recorded), over generated slots and frame sizes, against
//!   `DecodeEnv::init` / `apply_frame_dimensions` / `end_of_stream` /
//!   `advance_slice`;
//! - the stream open `FUN_801CF988` and the frame pump `FUN_801CFA14` over the
//!   disc's own dispatch slots and movies, the sectors fed through the `St`
//!   library's memory-source demux on the retail side, against
//!   `StrPlayer::open` / `next_frame` and the Iki converter;
//! - the MDEC output-word edit `FUN_801CFD84` and the slice-callback install
//!   `FUN_801CFEBC`.
//!
//! The MDEC DMA sync leaves beside them are `mdec_sync.rs`'s.

use crate::common::{Rng, disc, scus_cpu};
use psx_mdec::str_player::{
    Bitstream, DecodeEnv, FmvSlot, PumpIdle, Rect, StrPlayer, end_of_stream, mdec_control_flags,
    mdec_output_control,
};
use psx_retail_exec::{A0, A1, Cpu, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801C_E818;
const INIT: u32 = 0x801C_F8B0;
const POLL: u32 = 0x801C_F740;
const SLICE: u32 = 0x801C_F56C;
const OPEN: u32 = 0x801C_F988;
const PUMP: u32 = 0x801C_FA14;
const OUTPUT_WORD: u32 = 0x801C_FD84;
const SET_CALLBACK: u32 = 0x801C_FEBC;

/// `StGetNext`, `StFreeRing`, the library's `DecDCToutCallback`.
const ST_GET_NEXT: u32 = 0x8005_EF40;
const DCT_OUT_CALLBACK: u32 = 0x8005_FDE8;
/// The MDEC-out DMA kick and `LoadImage` the slice callback issues.
const KICK: u32 = 0x801C_FE00;
const LOAD_IMAGE: u32 = 0x8005_83C8;
/// The MDEC code-word upload `FUN_801CFD84` hands its word to.
const CODE_UPLOAD: u32 = 0x801C_FFDC;
/// The decoder reset and the CD seek `FUN_801CF988` makes.
const DCT_RESET: u32 = 0x801C_FC18;
const SEEK: u32 = 0x801C_FB94;
/// `CdReady`-shaped status poll the demux opens with.
const CD_POLL: u32 = 0x8005_BEAC;
const DEMUX: u32 = 0x8005_F024;

const CTX: u32 = 0x801D_19A0;
const INIT_LATCH: u32 = 0x801D_0D4C;
const DIM_CACHE: u32 = 0x801D_0D50;
const FIRST_SLICE: u32 = 0x801E_09F0;
const END_LATCH: u32 = 0x801E_09F8;
const IKI: u32 = 0x801E_09FC;
const DEMUX_PENDING: u32 = 0x801D_19D8;
/// The heap base the context's buffers and the ring are carved from.
const HEAP_PTR: u32 = 0x8007_B85C;
const HEAP: u32 = 0x8010_0000;
const SLOT: u32 = 0x801F_8000;
const HEADER: u32 = 0x801F_8100;
/// The play loop's stack arguments land at `sp + 0x10` / `+ 0x14`.
const STACK_TOP: u32 = 0x801F_FF00;

const MEM_SOURCE: u32 = 0x801C_ADC8;
const MEM_COUNTER: u32 = 0x801C_ADB8;
const NO_DRAIN: u32 = 0x801C_D208;
const SRC: u32 = 0x801F_9000;

/// A call the slice callback made: callee, buffer, then its words.
type Call = (u32, u32, u32, u32);

fn str_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(BASE, &disc()?.read_entry(970).ok()?);
    cpu.mem.wr32(HEAP_PTR, HEAP);
    cpu.step_budget = 50_000_000;
    Some(cpu)
}

fn write_slot(m: &mut Memory, s: &FmvSlot) {
    let words = [
        0,
        u32::from(s.colour),
        s.start_frame,
        s.end_frame,
        s.fb_x,
        s.fb_y,
        s.width,
        s.height,
    ];
    for (i, w) in words.iter().enumerate() {
        m.wr32(SLOT + 4 * i as u32, *w);
    }
}

fn rect(m: &mut Memory, at: u32) -> Rect {
    Rect {
        x: m.rd16(at) as i16,
        y: m.rd16(at + 2) as i16,
        w: m.rd16(at + 4) as i16,
        h: m.rd16(at + 6) as i16,
    }
}

/// The context as the port holds it, read off retail's struct.
fn retail_env(m: &mut Memory, colour: bool) -> DecodeEnv {
    let mut env = DecodeEnv::init(&FmvSlot::whole_file(16, 16));
    env.frame_buf = [rect(m, CTX + 0x18), rect(m, CTX + 0x20)];
    env.slice = rect(m, CTX + 0x2C);
    env.active_buf = m.rd32(CTX + 0x28) as usize;
    env.code_buf = m.rd32(CTX + 0x08) as usize;
    env.slice_buf = m.rd32(CTX + 0x14) as usize;
    env.first_slice = m.rd32(FIRST_SLICE) != 0;
    env.buffer_complete = m.rd32(CTX + 0x34) != 0;
    env.colour = m.rd32(CTX + 0x38) != 0;
    assert_eq!(env.colour, colour);
    env.set_slice_callback(true);
    env
}

/// The context after the init alone. Retail's init writes the rects'
/// origins only; their sizes come with the first frame poll, which the port
/// folds into its init from the slot, so those are taken from `port`.
fn initialised(m: &mut Memory, port: &DecodeEnv) -> DecodeEnv {
    let mut env = retail_env(m, port.colour);
    for b in 0..2 {
        env.frame_buf[b].w = port.frame_buf[b].w;
        env.frame_buf[b].h = port.frame_buf[b].h;
    }
    env.slice.h = port.slice.h;
    env
}

/// The play loop's setup of one slot, as `FUN_801CF098` makes it: the
/// first-slice flag up, then the context init on the slot's rects.
fn retail_init(cpu: &mut Cpu, s: &FmvSlot) {
    let units = |px: u32| psx_mdec::str_player::vram_units(px as i32, s.colour) as u32;
    write_slot(&mut cpu.mem, s);
    cpu.mem.wr32(FIRST_SLICE, 1);
    cpu.mem.wr32(INIT_LATCH, 1);
    cpu.mem.wr32(STACK_TOP + 0x10, s.fb_y + s.height);
    cpu.mem.wr32(STACK_TOP + 0x14, SLOT);
    let x = units(s.fb_x);
    assert_eq!(cpu.call(INIT, &[CTX, x, s.fb_y, x]), Stop::Returned, "init");
}

#[test]
fn the_decode_context_and_slice_walk_match_the_overlay() {
    differential!(prot(970, INIT), prot(970, POLL), prot(970, SLICE));
    let Some(mut cpu) = str_cpu() else {
        return;
    };
    let header = Rc::new(RefCell::new((0u16, 0u16, 0u32)));
    let h = header.clone();
    cpu.hook(ST_GET_NEXT, move |r, m| {
        let (w, ht, frame) = *h.borrow();
        m.wr32(HEADER + 8, frame);
        m.wr16(HEADER + 0x10, w);
        m.wr16(HEADER + 0x12, ht);
        m.wr32(r.r[A0], HEAP);
        m.wr32(r.r[A1], HEADER);
        r.r[V0] = 0;
    });
    // (callee, buffer, words) for the kick; (callee, buffer, rect x|y, w|h)
    // for the `LoadImage`, its rect read off the stack copy it is handed.
    let calls: Rc<RefCell<Vec<Call>>> = Rc::default();
    let c = calls.clone();
    cpu.hook(KICK, move |r, _| {
        c.borrow_mut().push((KICK, r.r[A0], r.r[A1], 0))
    });
    let c = calls.clone();
    cpu.hook(LOAD_IMAGE, move |r, m| {
        let at = r.r[A0];
        c.borrow_mut()
            .push((LOAD_IMAGE, r.r[A1], m.rd32(at), m.rd32(at + 4)))
    });
    cpu.mem.wr32(DEMUX_PENDING, 0);

    let mut rng = Rng(0x534C_4943);
    let (mut flips, mut partial, mut latched) = (0usize, 0usize, 0usize);
    for case in 0..400 {
        let slot = FmvSlot {
            colour: rng.below(2) == 0,
            start_frame: 1 + rng.below(4),
            end_frame: rng.below(40),
            fb_x: rng.below(4) * [0, 1, 7, 64][rng.below(4) as usize],
            fb_y: rng.below(64),
            width: 8 * (1 + rng.below(48)),
            height: 16 * (1 + rng.below(16)),
        };
        cpu.mem.load(CTX, &[0; 0x40]);
        cpu.mem.wr32(DIM_CACHE, 0);
        cpu.mem.wr32(DIM_CACHE + 4, 0);
        cpu.mem.wr32(END_LATCH, 0);
        retail_init(&mut cpu, &slot);
        let mut port = DecodeEnv::init(&slot);
        port.set_slice_callback(true);
        assert_eq!(initialised(&mut cpu.mem, &port), port, "case {case}: init");

        // A few frames, now and then with new dimensions.
        let (mut w, mut ht) = (slot.width as u16, slot.height as u16);
        for frame in 0..3u32 {
            if rng.below(3) == 0 {
                w = 8 * (1 + rng.below(48)) as u16;
                ht = 16 * (1 + rng.below(16)) as u16;
            }
            let number = slot.start_frame + frame * rng.below(30);
            *header.borrow_mut() = (w, ht, number);
            cpu.mem.wr32(END_LATCH, 0);
            assert_eq!(cpu.call(POLL, &[CTX, SLOT]), Stop::Returned);
            assert_eq!(cpu.regs.r[V0], HEAP, "the poll returns the frame");
            port.apply_frame_dimensions(w, ht);
            // Retail's latch has no zero guard; the port's is for its own
            // whole-file slot, which no retail slot is.
            let want = slot.end_frame <= number;
            assert_eq!(cpu.mem.rd32(END_LATCH) != 0, want, "case {case} latch");
            if slot.end_frame != 0 {
                assert_eq!(end_of_stream(number, slot.end_frame), want);
            }
            latched += usize::from(want);
            assert_eq!(
                retail_env(&mut cpu.mem, slot.colour),
                port,
                "case {case} poll"
            );
            assert_eq!(
                (cpu.mem.rd32(DIM_CACHE), cpu.mem.rd32(DIM_CACHE + 4)),
                (u32::from(w), u32::from(ht))
            );

            // Walk the slice cursor across a buffer and a bit.
            for step in 0..(2 + rng.below(70)) {
                calls.borrow_mut().clear();
                let ctx_bufs = [cpu.mem.rd32(CTX + 0x0C), cpu.mem.rd32(CTX + 0x10)];
                let before = rect(&mut cpu.mem, CTX + 0x2C);
                let slice_w = before.w;
                assert_eq!(cpu.call(SLICE, &[]), Stop::Returned);
                let got = port.advance_slice().expect("armed");
                let at = format!("case {case} frame {frame} step {step}");
                assert_eq!(retail_env(&mut cpu.mem, slot.colour), port, "{at}");
                // The kick into the next slice's staging buffer, then the
                // `LoadImage` of the finished slice out of its own.
                let r = got.load_rect;
                let mut want = Vec::new();
                if let Some(words) = got.kick_words {
                    want.push((KICK, ctx_bufs[port.slice_buf], words as u32, 0));
                }
                want.push((
                    LOAD_IMAGE,
                    ctx_bufs[got.load_buffer],
                    r.x as u16 as u32 | (r.y as u16 as u32) << 16,
                    r.w as u16 as u32 | (r.h as u16 as u32) << 16,
                ));
                assert_eq!(r, before, "{at}");
                assert_eq!(*calls.borrow(), want, "{at}");
                flips += usize::from(got.flipped);
                partial += usize::from(port.frame_buf[0].w % slice_w != 0);
            }
        }
    }
    assert!(
        flips > 300 && partial > 300 && latched > 100,
        "{flips} {partial} {latched}"
    );
}

/// One retail dispatch slot read off the overlay, with the movie it names.
fn retail_slots(cpu: &mut Cpu) -> Vec<(u32, FmvSlot, String)> {
    (0..9u32)
        .map(|i| {
            let at = 0x801D_0A6C + i * 0x20;
            let rec: [u8; 0x20] = cpu.mem.read(at, 0x20).try_into().unwrap();
            let name_at = cpu.mem.rd32(at);
            let path = cpu.mem.read(name_at, 24);
            let path: String = path
                .iter()
                .take_while(|&&b| b != b';' && b != 0)
                .map(|&b| b as char)
                .collect();
            (at, FmvSlot::from_record(&rec), path.replace('\\', "/"))
        })
        .collect()
}

#[test]
fn the_stream_open_and_frame_pump_match_the_overlay_on_the_disc_movies() {
    differential!(prot(970, OPEN), prot(970, PUMP));
    let Some(mut probe) = str_cpu() else {
        return;
    };
    let d = disc().expect("disc");
    let (mut pumped, mut ends) = (0usize, 0usize);
    for (k, (_, retail_slot, path)) in retail_slots(&mut probe).into_iter().enumerate() {
        // The segment, cut short so the run stays quick: the end latch is
        // what stops both sides.
        let mut slot = retail_slot;
        slot.end_frame = slot.end_frame.min(slot.start_frame + 12 + k as u32);
        let mut cpu = str_cpu().expect("disc");
        cpu.hook(DCT_RESET, |_, _| {});
        cpu.hook(SEEK, |_, _| {});
        let armed = Rc::new(RefCell::new(None));
        let a = armed.clone();
        cpu.hook(DCT_OUT_CALLBACK, move |r, _| {
            *a.borrow_mut() = Some((r.r[A0], r.r[A1]))
        });
        cpu.hook(CD_POLL, |r, m| {
            m.wr8(r.r[A1], 0);
            r.r[V0] = 0;
        });
        // The CD register pointers the demux pokes, onto scratch RAM.
        for (i, va) in [
            0x8007_9770u32,
            0x8007_9778,
            0x8007_977C,
            0x8007_9780,
            0x8007_9784,
        ]
        .iter()
        .enumerate()
        {
            cpu.mem.wr32(*va, 0x801F_A000 + 0x10 * i as u32);
        }
        cpu.mem.wr32(0x8007_97A0, 0x801F_A080);
        // The MDEC-in DMA status a colour stream's demux checks: idle.
        cpu.mem.wr32(0x8007_9790, 0x801F_A0C0);
        cpu.mem.wr32(IKI, 1);
        retail_init(&mut cpu, &slot);
        assert_eq!(cpu.call(OPEN, &[0x801F_B000, SLICE, SLOT]), Stop::Returned);
        assert_eq!(
            *armed.borrow(),
            Some((1, SLICE)),
            "the slice callback armed"
        );
        let mut player = StrPlayer::open(slot, Bitstream::Iki);
        assert_eq!(
            initialised(&mut cpu.mem, player.env()),
            *player.env(),
            "{path} open"
        );
        cpu.mem.wr32(MEM_SOURCE, SRC);
        cpu.mem.wr32(NO_DRAIN, 1);

        let (lba, size) =
            psx_iso::iso9660::find_path_in_image(d.image(), &path).expect("movie on the disc");
        let first = player.seek_sector_offset().max(0) as u32;
        let sectors = size.div_ceil(2048).saturating_sub(first);
        let data = d
            .read_disc_sectors(lba + first, sectors.min(14 * 10 + 30 * 10))
            .expect("sectors");
        for (n, s) in data.as_chunks::<2048>().0.iter().enumerate() {
            cpu.mem.wr32(MEM_COUNTER, 0);
            cpu.mem.load(SRC, s);
            assert_eq!(cpu.call(DEMUX, &[]), Stop::Returned);
            player.deliver_sector(s);
            assert_eq!(
                crate::st_ring::retail_cursors(&mut cpu.mem),
                player.ring().cursors(),
                "{path} sector {n}"
            );
            for i in 0..psx_mdec::str_player::RING_SLOTS {
                assert_eq!(
                    cpu.mem.rd16(HEAP + 32 * i as u32),
                    player.ring().slot_status(i),
                    "{path} sector {n} slot {i}"
                );
            }
            let frame = match player.next_frame() {
                Err(PumpIdle::NeedSectors) => {
                    // The pump's poll found nothing: retail's spins on
                    // `StGetNext` have the one side effect the port's single
                    // try has (a reader parked on the wrap marker restarts at
                    // slot 0), so run one on the retail ring.
                    assert_eq!(
                        cpu.call(ST_GET_NEXT, &[SLOT + 0x40, SLOT + 0x44]),
                        Stop::Returned
                    );
                    assert_ne!(cpu.regs.r[V0], 0, "{path} sector {n}: retail had a frame");
                    continue;
                }
                Err(PumpIdle::Finished) => break,
                Ok(f) => f,
            };
            let at = format!("{path} sector {n} frame {}", frame.frame_number);
            assert_eq!(cpu.call(PUMP, &[CTX, SLOT]), Stop::Returned, "{at}");
            assert_eq!(cpu.regs.r[V0] as i32, 0, "{at}: pump result");
            assert_eq!(retail_env(&mut cpu.mem, slot.colour), *player.env(), "{at}");
            assert_eq!(cpu.mem.rd32(END_LATCH) != 0, frame.is_last, "{at}");
            assert_eq!(frame.bitstream_kind, Bitstream::Iki);
            let codes = psx_mdec::iki_mdec_codes(&frame.bitstream).expect("Iki");
            let buf = cpu.mem.rd32(CTX + 4 * frame.code_buf as u32);
            let retail = cpu.mem.read(buf, codes.len() * 2);
            let port: Vec<u8> = codes.iter().flat_map(|c| c.to_le_bytes()).collect();
            assert!(retail == port, "{at}: the frame's MDEC codes");
            pumped += 1;
            ends += usize::from(frame.is_last);
            if frame.is_last {
                break;
            }
        }
    }
    assert!(pumped > 100 && ends == 9, "pumped {pumped} ends {ends}");
}

#[test]
fn the_mdec_output_word_matches_the_overlay() {
    differential!(prot(970, OUTPUT_WORD));
    let Some(mut cpu) = str_cpu() else {
        return;
    };
    let uploaded = Rc::new(RefCell::new(0u32));
    let u = uploaded.clone();
    cpu.hook(CODE_UPLOAD, move |r, _| *u.borrow_mut() = r.r[A1]);
    let mut rng = Rng(0x4D44_4543);
    for case in 0..2000 {
        let word = rng.next();
        let flags = if case % 2 == 0 {
            mdec_control_flags(rng.below(2) == 0)
        } else {
            rng.next()
        };
        cpu.mem.wr32(HEADER, word);
        assert_eq!(cpu.call(OUTPUT_WORD, &[HEADER, flags]), Stop::Returned);
        let want = mdec_output_control(word, flags);
        assert_eq!(cpu.mem.rd32(HEADER), want, "case {case}");
        assert_eq!(
            *uploaded.borrow(),
            want & 0xFFFF,
            "the upload takes the low half"
        );
    }
}

#[test]
fn the_slice_callback_install_matches_the_overlay() {
    differential!(prot(970, SET_CALLBACK));
    let Some(mut cpu) = str_cpu() else {
        return;
    };
    let installed = Rc::new(RefCell::new(None));
    let i = installed.clone();
    cpu.hook(DCT_OUT_CALLBACK, move |r, _| {
        *i.borrow_mut() = Some((r.r[A0], r.r[A1]))
    });
    let mut env = DecodeEnv::init(&FmvSlot::whole_file(320, 224));
    for handler in [SLICE, 0, SLICE, 0] {
        assert_eq!(cpu.call(SET_CALLBACK, &[handler]), Stop::Returned);
        // `DecDCToutCallback` on the MDEC-out channel, the handler as given.
        assert_eq!(*installed.borrow(), Some((1, handler)));
        env.set_slice_callback(handler != 0);
        assert_eq!(env.slice_callback_armed(), handler != 0);
    }
}
