//! The field actor visibility cull `FUN_801D79E8` (PROT 0897) against
//! `field_actor_culled`, over random focus pairs, region boxes, tile windows,
//! cull radii and positions on and around every edge, with the map-view fade
//! word up and down. The whole flag word `+0x10` is compared, and the floor
//! height `+0x16` that a visible actor with `+0x52 & 0x40` takes from the
//! floor sampler `FUN_80019278` (stubbed to a marker here) is held to the
//! port's verdict.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::field_npc_cull::field_actor_culled;
use legaia_engine_field::camera::FieldCullView;
use psx_retail_exec::Stop;

const CULL: u32 = 0x801D_79E8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FLOOR: u32 = 0x8001_9278;
const FADE: u32 = 0x8007_BAF4;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const BOX: u32 = 0x1F80_0384;
const WINDOW: u32 = 0x1F80_03E8;
const ACTOR: u32 = 0x8013_0000;
const MARKER: u16 = 0x5A5A;

#[test]
fn the_field_actor_cull_matches_retail() {
    differential!(prot(897, CULL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().unwrap().read_entry(897).expect("PROT 0897"),
    );
    cpu.stub(FLOOR, u32::from(MARKER));
    let mut rng = Rng(0x4355_4C4C);
    let (mut culled, mut visible, mut faded, mut floored) = (0, 0, 0, 0);
    for run in 0..100_000 {
        let attr_box: [u8; 4] = {
            let (x0, z0) = (rng.below(0x60) as u8, rng.below(0x60) as u8);
            [
                x0,
                z0,
                x0 + rng.below(0x40) as u8,
                z0 + rng.below(0x40) as u8,
            ]
        };
        let window: [i8; 4] = [
            -(rng.below(12) as i8),
            -(rng.below(12) as i8),
            rng.below(12) as i8,
            rng.below(16) as i8,
        ];
        // Focus near the box, stored negated.
        let fx = (i32::from(attr_box[0]) + rng.below(0x20) as i32) * 128 + rng.below(128) as i32;
        let fz = (i32::from(attr_box[1]) + rng.below(0x20) as i32) * 128 + rng.below(128) as i32;
        let view = FieldCullView {
            focus_stored: [-fx, -fz],
            attr_box,
            window,
        };
        let spread = [0x600, 0x1400][rng.below(2) as usize];
        let x = (fx + rng.below(spread) as i32 - spread as i32 / 2) as i16;
        let z = (fz + rng.below(spread) as i32 - spread as i32 / 2) as i16;
        let radius = [0i16, 0, 1, 3, -2, rng.next() as i16 >> 12][rng.below(6) as usize];
        let fade = rng.below(8) == 0;
        let flags = rng.next();
        let f52 = rng.next() as u16;
        let y = rng.next() as u16;

        cpu.mem
            .wr32(FADE, if fade { 1 + rng.below(0x100) } else { 0 });
        cpu.mem.wr32(FOCUS_X, view.focus_stored[0] as u32);
        cpu.mem.wr32(FOCUS_Z, view.focus_stored[1] as u32);
        cpu.mem.load(BOX, &attr_box);
        cpu.mem.load(WINDOW, &window.map(|v| v as u8));
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x16, y);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        cpu.mem.wr16(ACTOR + 0x52, f52);
        cpu.mem.wr16(ACTOR + 0x58, radius as u16);
        assert_eq!(cpu.call(CULL, &[ACTOR]), Stop::Returned, "run {run}");

        let port = field_actor_culled(x, z, radius, fade, &view);
        let label = format!("run {run}: ({x}, {z}) r {radius} fade {fade} {view:?}");
        let want = if port { flags | 2 } else { flags & !2 };
        assert_eq!(cpu.mem.rd32(ACTOR + 0x10), want, "{label}");
        let floor = !port && !fade && f52 & 0x40 != 0;
        let want_y = if floor { MARKER } else { y };
        assert_eq!(cpu.mem.rd16(ACTOR + 0x16), want_y, "{label}");
        culled += u32::from(port);
        visible += u32::from(!port && !fade);
        faded += u32::from(fade);
        floored += u32::from(floor);
    }
    assert!(
        culled > 20_000 && visible > 5_000 && faded > 5_000 && floored > 2_000,
        "{culled} {visible} {faded} {floored}"
    );
}
