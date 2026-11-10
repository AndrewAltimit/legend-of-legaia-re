//! The SCUS window emitter `FUN_8002C69C` on the dialog pager's reading-box
//! skin (`0x61`, staged in `gp+0x14C` by `FUN_80034B6C`) against
//! `legaia_engine_ui::{dialog_window_fill_rect, dialog_window_footprint}`
//! and the fill colours `title_pak::OVERLAY_SYSTEM_UI_DIALOG_FILL_*`: random
//! centre rects, every packet the emitter writes decoded. The fill is two
//! identical semi-transparent `POLY_G4`s over the fill rect, top edge one
//! colour and bottom edge the other; the border sprites cover exactly the
//! footprint. The border's tile-by-tile composition is the atlas's and is
//! not compared here.

use crate::common::{Rng, scus_cpu};
use legaia_engine_ui::{dialog_window_fill_rect, dialog_window_footprint};
use legaia_ui_formats::title_pak::{
    OVERLAY_SYSTEM_UI_DIALOG_FILL_BOT_RGB, OVERLAY_SYSTEM_UI_DIALOG_FILL_TOP_RGB,
};
use psx_retail_exec::{Cpu, Stop};

const EMIT: u32 = 0x8002_C69C;
const STYLE: u32 = 0x8007_B318 + 0x14C;
const READING_BOX: u32 = 0x61;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;

fn words(cpu: &mut Cpu) -> Vec<Vec<u32>> {
    let end = cpu.mem.rd32(POOL_PTR);
    let mut p = POOL;
    let mut out = Vec::new();
    while p < end {
        let n = u32::from(cpu.mem.rd8(p + 3));
        out.push((0..=n).map(|k| cpu.mem.rd32(p + 4 * k)).collect());
        p += 4 * (n + 1);
    }
    assert_eq!(p, end);
    out
}

#[test]
fn the_reading_box_fills_and_frames_where_the_port_does() {
    differential!(scus(EMIT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5749_4E45);
    for case in 0..2000 {
        let rect = (
            rng.below(200) as i32,
            rng.below(160) as i32,
            16 + rng.below(240) as i32,
            8 + rng.below(120) as i32,
        );
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        for i in 0..16 {
            cpu.mem.wr32(OT + 4 * i, 0x00FF_FFFF);
        }
        cpu.mem.wr32(STYLE, READING_BOX);
        let args = [rect.0, rect.1, rect.2, rect.3].map(|v| v as u32);
        assert_eq!(cpu.call(EMIT, &args), Stop::Returned, "case {case}");
        let packets = words(&mut cpu);

        // The fill: two `POLY_G4`s, semi-transparent.
        let fills: Vec<&Vec<u32>> = packets.iter().filter(|w| w[1] >> 24 == 0x3B).collect();
        assert_eq!(fills.len(), 2, "case {case}: two fill passes");
        let (fx, fy, fw, fh) = dialog_window_fill_rect(rect);
        let rgb = |c: (u8, u8, u8)| u32::from(c.0) | u32::from(c.1) << 8 | u32::from(c.2) << 16;
        let (top, bot) = (
            rgb(OVERLAY_SYSTEM_UI_DIALOG_FILL_TOP_RGB),
            rgb(OVERLAY_SYSTEM_UI_DIALOG_FILL_BOT_RGB),
        );
        for f in &fills {
            let xy = |k: usize| {
                (
                    f[2 + 2 * k] as i16 as i32,
                    (f[2 + 2 * k] >> 16) as i16 as i32,
                )
            };
            let colour = |k: usize| f[1 + 2 * k] & 0xFF_FFFF;
            let mut corners: Vec<(i32, i32, u32)> =
                (0..4).map(|k| (xy(k).0, xy(k).1, colour(k))).collect();
            corners.sort();
            let mut want = vec![
                (fx, fy, top),
                (fx + fw, fy, top),
                (fx, fy + fh, bot),
                (fx + fw, fy + fh, bot),
            ];
            want.sort();
            assert_eq!(corners, want, "case {case}: the fill quad");
        }

        // The border: sprites (`0x64..0x67`, x/y word, uv/clut word, w/h
        // word) whose union is the footprint.
        let (bx, by, bw, bh) = dialog_window_footprint(rect);
        let mut bounds = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for w in packets.iter().filter(|w| (w[1] >> 24) & 0xFC == 0x64) {
            let (x, y) = (w[2] as i16 as i32, (w[2] >> 16) as i16 as i32);
            let (sw, sh) = ((w[4] & 0xFFFF) as i32, (w[4] >> 16) as i32);
            bounds = (
                bounds.0.min(x),
                bounds.1.min(y),
                bounds.2.max(x + sw),
                bounds.3.max(y + sh),
            );
        }
        assert_eq!(
            bounds,
            (bx, by, bx + bw, by + bh),
            "case {case}: the border's footprint for {rect:?}"
        );
    }
}
