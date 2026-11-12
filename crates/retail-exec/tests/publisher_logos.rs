//! The boot overlay's publisher-logo draws (PROT 0895) against
//! `publisher_logos::LOGO_QUADS`: the SCEA pair `FUN_801D0868`, the PROKION
//! pair `FUN_801D08F0` and the emitter they share, `FUN_801CFBB8`, called
//! directly for Contrail at the sequencer's centre `(320, 232)`. Each
//! `POLY_GT4` the emitter writes at the scratchpad packet cursor
//! (`0x1F8003A0`) is read back as its screen rectangle and its texture
//! rectangle, at every brightness level, with the vertex colour the port's
//! fade note gives, `0xFF * level >> 8`.

use crate::common::{disc, scus_cpu};
use legaia_engine_title::publisher_logos::{
    LOGO_CONTRAIL, LOGO_PROKION, LOGO_QUADS, LOGO_SCEA, LogoQuad,
};
use psx_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const EMIT: u32 = 0x801C_FBB8;
const SCEA_PAIR: u32 = 0x801D_0868;
const PROKION_PAIR: u32 = 0x801D_08F0;
const ADD_PRIM: u32 = 0x8003_D2C4;
const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8014_0000;
const OT: u32 = 0x8015_0000;
const STACK_ARGS: u32 = 0x801F_FF00 + 0x10;

/// The quads written since the cursor stood at `PACKETS`, with each one's
/// first-vertex colour.
fn quads(cpu: &mut Cpu) -> Vec<(LogoQuad, [u8; 3])> {
    let end = cpu.mem.rd32(PACKET_CURSOR);
    let mut out = Vec::new();
    let mut p = PACKETS;
    while p < end {
        let m = &mut cpu.mem;
        assert_eq!(m.rd8(p + 7) & 0xFC, 0x3C, "a POLY_GT4");
        let (x0, y0) = (m.rd16(p + 8) as i16, m.rd16(p + 0xA) as i16);
        let x1 = m.rd16(p + 0x14) as i16;
        let y2 = m.rd16(p + 0x22) as i16;
        let (u0, v0) = (m.rd8(p + 0xC), m.rd8(p + 0xD));
        let u1 = m.rd8(p + 0x18);
        let v2 = m.rd8(p + 0x25);
        out.push((
            LogoQuad {
                src: (
                    u32::from(u0),
                    u32::from(v0),
                    u32::from(u1.wrapping_sub(u0)),
                    u32::from(v2.wrapping_sub(v0)),
                ),
                dst: (
                    i32::from(x0),
                    i32::from(y0),
                    (x1 - x0) as u32,
                    (y2 - y0) as u32,
                ),
            },
            [m.rd8(p + 4), m.rd8(p + 5), m.rd8(p + 6)],
        ));
        p += 0x34;
    }
    out
}

#[test]
fn the_logo_quads_are_the_retail_ones() {
    differential!(
        prot(895, SCEA_PAIR),
        prot(895, PROKION_PAIR),
        prot(895, EMIT)
    );
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(895).expect("PROT 0895"));
    cpu.stub(ADD_PRIM, 0);
    cpu.mem.wr32(OT_PTR, OT);
    for level in [0u32, 1, 0x40, 0x7F, 0x80] {
        for (entry, logo) in [
            (SCEA_PAIR, LOGO_SCEA),
            (PROKION_PAIR, LOGO_PROKION),
            (EMIT, LOGO_CONTRAIL),
        ] {
            cpu.mem.wr32(PACKET_CURSOR, PACKETS);
            let stop = if entry == EMIT {
                cpu.mem.wr32(STACK_ARGS, level);
                cpu.mem.wr32(STACK_ARGS + 4, 0x1000);
                cpu.call(EMIT, &[0xA, 320, 232, 4])
            } else {
                cpu.call(entry, &[level])
            };
            assert_eq!(stop, Stop::Returned);
            let got = quads(&mut cpu);
            let want: Vec<LogoQuad> = LOGO_QUADS[logo].to_vec();
            let geometry: Vec<LogoQuad> = got.iter().map(|(q, _)| *q).collect();
            assert_eq!(geometry, want, "logo {logo} level {level:#x}");
            let shade = ((0xFF * level) >> 8) as u8;
            for (_, rgb) in &got {
                assert_eq!(*rgb, [shade; 3], "logo {logo} level {level:#x}");
            }
        }
    }
}

/// The atlas the port builds in place of the `READ INIT` body's uploads
/// (`the_read_init_body_matches_retail` runs the body): the init pak decodes
/// to every logo the sequencer draws, and the atlas gives each one a rect of
/// its own size.
#[test]
fn the_init_pak_carries_every_logo_the_port_draws() {
    not_a_differential!(
        "checks the atlas the port builds from the init pak in place of the VRAM uploads; runs no routine"
    );
    use legaia_engine_title::publisher_logos::{LOGO_COUNT, build_atlas_from_init_pak};
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pak = d.read_entry(895).expect("PROT 0895");
    let atlas = build_atlas_from_init_pak(&pak).expect("the init pak's logos");
    assert_eq!(atlas.rects.len(), LOGO_COUNT);
    let mut y = 0;
    for (i, r) in atlas.rects.iter().enumerate() {
        assert!(r.2 > 0 && r.3 > 0, "logo {i}: {r:?}");
        assert_eq!(r.1, y, "logo {i} stacks under the one before");
        y += r.3;
    }
    // The white block the wash samples sits under the logos, opaque white.
    let (wx, wy, ww, wh) = atlas.white;
    assert_eq!(wy, y, "the white block stacks under the logos");
    assert_eq!(y + wh, atlas.height);
    for row in wy..wy + wh {
        for col in wx..wx + ww {
            let o = ((row * atlas.width + col) * 4) as usize;
            assert_eq!(atlas.rgba[o..o + 4], [0xFF; 4], "white at ({col}, {row})");
        }
    }
    // Every logo quad the sequencer draws samples inside its logo's rect.
    for (logo, quads) in LOGO_QUADS.iter().enumerate() {
        let r = atlas.rects[logo];
        for q in quads.iter() {
            assert!(
                q.src.0 + q.src.2 <= r.2 && q.src.1 + q.src.3 <= r.3,
                "logo {logo}: quad {q:?} within {r:?}"
            );
        }
    }
}

/// The sequencer `FUN_801CEFD4`, its body.
const SEQUENCER: u32 = 0x801C_EFD4;
/// The screen fade the PROKION fade-out draws over the held logo.
const SCREEN_FADE: u32 = 0x801D_0460;
/// The CD-load word state 5 raises and the Contrail state waits on.
const LOAD_PENDING: u32 = 0x801F_3EA4;
const COUNTER: u32 = 0x801F_3EA8;
const DONE_WORD: u32 = 0x801F_3EB0;
/// `r0` of the first `DRAWENV` (`0x8007BF44 + 0x19`).
const DRAWENV_BG: u32 = 0x8007_BF5D;
const BOOT_ACTOR: u32 = 0x8016_0000;

/// What one sequencer frame drew: the logo, the level it handed the
/// emitter, and the screen fade's level when one was drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    logo: usize,
    level: u32,
    fade: Option<u32>,
}

/// The sequencer run frame by frame from its state `0` at frame step `1`,
/// against `PublisherLogosSession` ticked then drawn as the front end does:
/// the settles' black frames, the logo each frame shows and its level, and
/// the frame the done bit rises. The CD-load word the Contrail state waits on
/// is cleared each frame, the stand-in for the load callback.
#[test]
fn the_sequencer_paces_the_logos_as_the_session_does() {
    differential!(prot(895, SEQUENCER));
    use legaia_engine_title::publisher_logos::PublisherLogosSession;
    use std::cell::RefCell;
    use std::rc::Rc;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(895).expect("PROT 0895"));
    let frame = Rc::new(RefCell::new(Vec::<Drawn>::new()));
    for (va, logo) in [(SCEA_PAIR, LOGO_SCEA), (PROKION_PAIR, LOGO_PROKION)] {
        let frame = frame.clone();
        cpu.hook(va, move |r, _| {
            frame.borrow_mut().push(Drawn {
                logo,
                level: r.r[psx_retail_exec::A0],
                fade: None,
            })
        });
    }
    {
        let frame = frame.clone();
        cpu.hook(EMIT, move |r, m| {
            let level = m.rd32(r.r[psx_retail_exec::SP] + 0x10);
            frame.borrow_mut().push(Drawn {
                logo: LOGO_CONTRAIL,
                level,
                fade: None,
            })
        });
    }
    {
        let frame = frame.clone();
        cpu.hook(SCREEN_FADE, move |r, _| {
            // Drawn ahead of the logo it covers; marked as a logo-less
            // entry and folded onto the logo below. A grey (`r = g = b`)
            // on ordering-table slot 0 under ABR `B + F`.
            let g = r.r[psx_retail_exec::A2] & 0xFF;
            assert_eq!(r.r[psx_retail_exec::A2], g * 0x1_0101, "a grey wash");
            assert_eq!(r.r[psx_retail_exec::A0], 0, "OT slot 0");
            assert_eq!(
                r.r[psx_retail_exec::A1],
                u32::from(legaia_engine_title::publisher_logos::WASH_ABR),
                "the wash's ABR"
            );
            frame.borrow_mut().push(Drawn {
                logo: usize::MAX,
                level: 0,
                fade: Some(r.r[psx_retail_exec::A2] & 0xFF),
            });
        });
    }
    for va in [
        0x8005_8068,
        0x801D_0828,
        0x8001_DAF8,
        0x8005_8298,
        0x8002_5CB4,
        0x801C_FA78,
        0x8003_F120,
        0x801C_FF68,
    ] {
        cpu.stub(va, 0);
    }
    for o in (0..0x100).step_by(4) {
        cpu.mem.wr32(BOOT_ACTOR + o, 0);
    }
    cpu.mem.wr16(BOOT_ACTOR + 0x1A, 0);
    cpu.mem.wr32(COUNTER, 0);
    cpu.mem.wr32(DONE_WORD, 0);
    cpu.mem.wr8(0x1F80_0393, 1);
    let mut port = PublisherLogosSession::new();
    let mut frames = 0;
    let mut seen = Vec::new();
    loop {
        frame.borrow_mut().clear();
        cpu.mem.wr32(LOAD_PENDING, 0);
        assert_eq!(cpu.call(SEQUENCER, &[BOOT_ACTOR]), Stop::Returned);
        port.tick();
        let state = cpu.mem.rd16(BOOT_ACTOR + 0x1A);
        let mut drawn = Vec::new();
        let mut fade = None;
        for d in frame.borrow().iter() {
            if d.logo == usize::MAX {
                fade = d.fade;
            } else {
                drawn.push(Drawn { fade, ..*d });
            }
        }
        let at = format!("frame {frames} state {state}");
        let retail_done = cpu.mem.rd32(DONE_WORD) & 2 != 0;
        assert_eq!(retail_done, port.is_done(), "the sequence's end, {at}");
        if retail_done {
            break;
        }
        let level = u32::from(port.retail_level());
        // The `r0 / g0 / b0` bytes of the first `DRAWENV`: white through
        // the tail settles, black before them.
        let bg = cpu.mem.rd8(DRAWENV_BG);
        let Some(&d) = drawn.first() else {
            // A settle: no logo drawn, and the port draws none. Black ahead
            // of the logos; white (a whole-stage `0xFF` wash) after them.
            assert_eq!(level, 0, "no logo, {at}");
            let white = state >= 11;
            assert_eq!(bg, if white { 0xFF } else { 0 }, "the background, {at}");
            assert_eq!(
                port.screen_wash().map(|w| (w.rect, w.level)),
                white.then_some(((0, 0, 640, 480), 0xFF)),
                "the tail's white, {at}"
            );
            frames += 1;
            continue;
        };
        assert_eq!(drawn.len(), 1, "one logo a frame, {at}");
        match d.fade {
            // A logo at level `0` is a black frame, whichever logo each
            // side names (the frame a logo's step rolls into the next).
            None => {
                assert_eq!(d.level, level, "level, {at}");
                assert_eq!(port.screen_wash(), None, "no wash, {at}");
                if level != 0 {
                    assert_eq!(d.logo, port.current_logo(), "logo, {at}");
                }
            }
            // State 10: PROKION held at full under the additive wash, whose
            // grey the port draws frame for frame.
            Some(f) => {
                assert_eq!((d.logo, d.level), (LOGO_PROKION, 0x80), "{at}");
                assert_eq!(port.current_logo(), LOGO_PROKION, "{at}");
                assert_eq!(level, 0x80, "the logo stays at full, {at}");
                let wash = port.screen_wash().expect("the port's wash");
                assert_eq!(wash.level, f as u8, "the wash's grey, {at}");
                assert_eq!(
                    wash.rect,
                    legaia_engine_title::publisher_logos::WASH_RECT,
                    "{at}"
                );
            }
        }
        if d.level != 0 && seen.last() != Some(&d.logo) {
            seen.push(d.logo);
        }
        frames += 1;
        assert!(frames < 2000, "the sequence does not end");
    }
    assert_eq!(seen, [LOGO_SCEA, LOGO_CONTRAIL, LOGO_PROKION]);
}

/// `FUN_801CE9C0`, the mode-16 `READ INIT` body.
const READ_INIT: u32 = 0x801C_E9C0;
const READ_INIT_END: u32 = 0x801C_ECD0;
const ENTRY_WORD: u32 = 0x8007_BB00;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Setup {
    Display(u32),
    Clear((i16, i16, i16, i16)),
    /// File offset of the TIM, its CLUT and pixel destinations, 8bpp.
    Upload(usize, (u16, u16), (u16, u16), bool),
    ClutRow((i16, i16, i16, i16), [u16; 2]),
}

/// The body run under every combination of the four words its skip arms
/// read, every call outside it stubbed and the display, clear, upload and
/// CLUT-row calls read back at the call, against
/// `publisher_logos::read_init` and the setup constants it documents: the
/// entry word, the done word, the load permit, the uploads in their order
/// with their VRAM placements, the clear rect, the CLUT row and the next
/// game mode. The six sprite descriptors are then read back through the
/// placements, each naming the logo `LOGO_QUADS` draws it for.
#[test]
fn the_read_init_body_matches_retail() {
    differential!(prot(895, READ_INIT));
    use legaia_engine_title::publisher_logos::{
        LOGO_UPLOAD_ORDER, LOGO_VRAM, LOGOS_DONE_BIT, READ_INIT_CLEAR, READ_INIT_CLUT_HEAD,
        READ_INIT_CLUT_ROW, READ_INIT_DISPLAY, READ_INIT_NEXT_MODE, ReadInitFlags, read_init,
    };
    use psx_retail_exec::{A0, A1};
    use std::cell::RefCell;
    use std::rc::Rc;
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let pak = d.read_entry(895).expect("PROT 0895");
    let log = Rc::new(RefCell::new(Vec::<Setup>::new()));
    cpu.stub_outside = Some((READ_INIT, READ_INIT_END));
    let rect = |m: &mut psx_retail_exec::Memory, a: u32| {
        (
            m.rd16(a) as i16,
            m.rd16(a + 2) as i16,
            m.rd16(a + 4) as i16,
            m.rd16(a + 6) as i16,
        )
    };
    {
        let l = log.clone();
        cpu.hook(0x8001_DAF8, move |r, _| {
            l.borrow_mut().push(Setup::Display(r.r[A0]))
        });
        let l = log.clone();
        cpu.hook(0x8005_8298, move |r, m| {
            l.borrow_mut().push(Setup::Clear(rect(m, r.r[A0])))
        });
        let l = log.clone();
        cpu.hook(0x8005_83C8, move |r, m| {
            let head = [m.rd16(r.r[A1]), m.rd16(r.r[A1] + 2)];
            l.borrow_mut().push(Setup::ClutRow(rect(m, r.r[A0]), head));
        });
        let l = log.clone();
        cpu.hook(0x8001_98E0, move |r, m| {
            let tim = r.r[A0];
            let clut_len = m.rd32(tim + 8);
            let pix = tim + 8 + clut_len;
            l.borrow_mut().push(Setup::Upload(
                (tim - OVERLAY_BASE) as usize,
                (m.rd16(tim + 12), m.rd16(tim + 14)),
                (m.rd16(pix + 4), m.rd16(pix + 6)),
                m.rd32(tim + 4) & 7 == 1,
            ));
        });
    }
    let offsets: Vec<usize> = legaia_ui_formats::init_pak::parse(&pak)
        .expect("the init pak")
        .logos
        .iter()
        .map(|l| l.file_offset)
        .collect();
    let want_setup: Vec<Setup> = {
        let up = |i: usize| {
            let logo = LOGO_UPLOAD_ORDER[i];
            let v = LOGO_VRAM[logo];
            Setup::Upload(offsets[logo], v.clut, v.pixels, v.bpp8)
        };
        vec![
            up(0),
            up(1),
            Setup::Display(u32::from(READ_INIT_DISPLAY)),
            Setup::Clear(READ_INIT_CLEAR),
            up(2),
            up(3),
            Setup::ClutRow(READ_INIT_CLUT_ROW, READ_INIT_CLUT_HEAD),
        ]
    };
    let mut skipped = 0;
    for bits in 0..32u32 {
        let f = ReadInitFlags {
            debug_word: if bits & 1 != 0 { 0x8000_0000 } else { 0 },
            build_mode: if bits & 2 != 0 { 1 } else { 0 },
            pad: if bits & 4 != 0 { 0x0001 } else { 0x0100 },
            dev_word: if bits & 8 != 0 { 1 } else { 0 },
        };
        // The permit word's bit 0 starts down or already up.
        let permit_in = (bits >> 4) & 1;
        cpu.mem.load(OVERLAY_BASE, &pak);
        cpu.mem.wr32(0x8007_B98C, f.debug_word);
        cpu.mem.wr16(0x8007_B8C2, f.build_mode as u16);
        cpu.mem.wr32(0x8007_B850, f.pad);
        cpu.mem.wr32(0x8007_B868, f.dev_word);
        cpu.mem.wr32(ENTRY_WORD, 0xDEAD);
        cpu.mem.wr32(DONE_WORD, 0xDEAD);
        cpu.mem.wr32(LOAD_PENDING, 0x40 | permit_in);
        cpu.mem.wr16(0x8007_B83C, 0x10);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(READ_INIT, &[]), Stop::Returned);
        let port = read_init(f);
        let at = format!("{f:?}");
        assert_eq!(
            cpu.mem.rd32(ENTRY_WORD),
            port.entry_word,
            "entry word, {at}"
        );
        assert_eq!(cpu.mem.rd32(DONE_WORD), port.done, "done word, {at}");
        assert_eq!(
            cpu.mem.rd32(LOAD_PENDING),
            0x40 | permit_in | u32::from(port.permit),
            "permit, {at}"
        );
        assert_eq!(cpu.mem.rd16(0x8007_B83C), READ_INIT_NEXT_MODE, "{at}");
        assert_eq!(*log.borrow(), want_setup, "setup, {at}");
        skipped += u32::from(port.done & LOGOS_DONE_BIT != 0);
    }
    assert!(skipped > 0 && skipped < 32, "{skipped}");
    // The retail boot plays the logos.
    assert_eq!(read_init(ReadInitFlags::RETAIL).done, 0);

    // Each descriptor's tpage / clut names the logo whose quads use it.
    let quads_of = |desc: usize| match desc {
        0 | 5 => LOGO_PROKION,
        1 => legaia_engine_title::publisher_logos::LOGO_WARNING,
        2 | 3 => LOGO_SCEA,
        _ => LOGO_CONTRAIL,
    };
    for desc in 0..6 {
        let o = 0x24E84 + 20 * desc;
        let tpage = u16::from_le_bytes([pak[o + 4], pak[o + 5]]);
        let clut = u16::from_le_bytes([pak[o + 6], pak[o + 7]]);
        let v = LOGO_VRAM[quads_of(desc)];
        assert_eq!(
            (tpage & 0x9F, clut),
            (v.tpage(), v.clut_word()),
            "descriptor {desc}"
        );
    }
}
