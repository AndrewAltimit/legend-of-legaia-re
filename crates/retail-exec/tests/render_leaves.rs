//! Two `SCUS_942.54` render leaves against `legaia_render_kernels`: the
//! gouraud colour spread `FUN_80036C4C` (`spread_prim_colors`) over whole
//! packets, and the display X mirror `FUN_800597C8` (`screen_x_mirror`) over
//! every orientation byte and mirror flag.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::display_mirror::{ScreenOrient, screen_x_mirror};
use legaia_render_kernels::gte::raster::spread_prim_colors;
use psx_retail_exec::{Stop, V0};

const SPREAD: u32 = 0x8003_6C4C;
const MIRROR: u32 = 0x8005_97C8;
const PACKET: u32 = 0x8010_0000;
const COLOURS: u32 = 0x8010_0100;
const ENTRY: u32 = 0x8010_0200;

#[test]
fn the_gouraud_colour_spread_matches_retail() {
    differential!(scus(SPREAD));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5350_5244);
    for case in 0..2000 {
        let mut packet: Vec<u8> = (0..0x30).map(|_| rng.next() as u8).collect();
        let colours: Vec<u8> = (0..0x10).map(|_| rng.next() as u8).collect();
        let count = [3usize, 4, 0, 5, 2][rng.below(5) as usize];
        cpu.mem.load(PACKET, &packet);
        cpu.mem.load(COLOURS, &colours);
        assert_eq!(
            cpu.call(SPREAD, &[PACKET, COLOURS, count as u32]),
            Stop::Returned
        );
        spread_prim_colors(&mut packet, &colours, count);
        assert_eq!(
            cpu.mem.read(PACKET, 0x30),
            packet,
            "case {case} count {count}"
        );
    }
}

#[test]
fn the_display_x_mirror_matches_retail() {
    differential!(scus(MIRROR));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x4D49_5252);
    for case in 0..4000 {
        let orient = [0u8, 1, 2, 3, 0xFF][rng.below(5) as usize];
        let mirror = [0u8, 1, 0x80][rng.below(3) as usize];
        let (x, w) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr8(0x8007_8D54, orient);
        cpu.mem.wr8(0x8007_8D57, mirror);
        cpu.mem.wr16(ENTRY, x as u16);
        cpu.mem.wr16(ENTRY + 4, w as u16);
        assert_eq!(cpu.call(MIRROR, &[ENTRY]), Stop::Returned);
        let port = screen_x_mirror(ScreenOrient::from_byte(orient), mirror != 0, x, w);
        assert_eq!(
            cpu.regs.r[V0] as i32, port,
            "case {case}: orient {orient} mirror {mirror} x {x} w {w}"
        );
    }
}
