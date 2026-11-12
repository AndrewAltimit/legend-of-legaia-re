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

/// `FUN_801CE9C0`, the mode-16 `READ INIT` body, is not run: it sets the
/// 640x480 display, clears VRAM, initialises the pads, spawns two boot
/// actors and uploads the logo TIMs it has just read off the CD - GPU,
/// controller and CD work the interpreter has nothing to compare. What the
/// port takes from it is the logos themselves, which this holds: the init
/// pak decodes to every logo the sequencer draws, and the atlas the port
/// builds from it gives each one a rect of its own size.
#[test]
fn the_init_pak_carries_every_logo_the_port_draws() {
    not_a_differential!(
        "FUN_801CE9C0 is display, VRAM, pad and CD setup with TIM uploads; checks the logos the port builds from its pak"
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
    assert_eq!(y, atlas.height);
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
            // entry and folded onto the logo below.
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
        let Some(&d) = drawn.first() else {
            // A settle: nothing drawn, and the port's frame is black.
            assert_eq!(level, 0, "a black frame, {at}");
            frames += 1;
            continue;
        };
        assert_eq!(drawn.len(), 1, "one logo a frame, {at}");
        match d.fade {
            // A logo at level `0` is a black frame, whichever logo each
            // side names (the frame a logo's step rolls into the next).
            None => {
                assert_eq!(d.level, level, "level, {at}");
                if level != 0 {
                    assert_eq!(d.logo, port.current_logo(), "logo, {at}");
                }
            }
            // State 10: PROKION held at full under the blend quad, which
            // the port stands in for by fading the logo over the same span
            // (`docs/subsystems/boot.md`); the quad's level and the port's
            // drop run on one schedule.
            Some(f) => {
                assert_eq!((d.logo, d.level), (LOGO_PROKION, 0x80), "{at}");
                assert_eq!(port.current_logo(), LOGO_PROKION, "{at}");
                let drop = 2 * (0x80 - level);
                assert!(
                    drop.abs_diff(f.min(0x100)) <= 10,
                    "blend {f:#x} against the port's drop {drop:#x}, {at}"
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
