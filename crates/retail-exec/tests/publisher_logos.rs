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
            let shade = (0xFF * level >> 8) as u8;
            for (_, rgb) in &got {
                assert_eq!(*rgb, [shade; 3], "logo {logo} level {level:#x}");
            }
        }
    }
}
