//! The world-map top view's screen dim `FUN_801E75DC` (PROT 0897) against
//! `emit_screen_dim`: the three packets it links (two `DR_MODE`s built by the
//! SCUS `SetDrawMode` around a black semi-transparent `POLY_F4`), decoded
//! back from the bytes, their link order and slot, and the prim-pool cursor.

use crate::common::{disc, scus_cpu};
use legaia_engine_field::world_map_dim::{ScreenDimPass, emit_screen_dim};
use psx_retail_exec::{A0, A1, Stop};

const DIM: u32 = 0x801E_75DC;
const OVERLAY_BASE: u32 = 0x801C_E818;
const OT_LINK: u32 = 0x8003_D2C4;
const PACKET_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8015_0000;

#[test]
fn the_screen_dim_matches_retail() {
    differential!(prot(897, DIM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    let links = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    {
        let links = links.clone();
        cpu.hook(OT_LINK, move |r, _| {
            links.borrow_mut().push((r.r[A0], r.r[A1]))
        });
    }
    let ot = 0x8016_0040;
    cpu.mem.wr32(OT_PTR, ot);
    cpu.mem.wr32(PACKET_PTR, PACKETS);
    cpu.mem.load(PACKETS, &[0xA5; 0x40]);
    assert_eq!(cpu.call(DIM, &[]), Stop::Returned);
    let pass: ScreenDimPass = emit_screen_dim();
    let got = links.borrow().clone();
    assert_eq!(
        got,
        vec![
            (ot + 8, PACKETS),
            (ot + 8, PACKETS + 12),
            (ot + 8, PACKETS + 36)
        ]
    );
    assert_eq!(
        cpu.mem.rd32(PACKET_PTR),
        PACKETS + ScreenDimPass::POOL_BYTES
    );
    for (p, m) in [(PACKETS, pass.mode_before), (PACKETS + 36, pass.mode_after)] {
        let w = cpu.mem.rd32(p + 4);
        assert_eq!(w >> 24, 0xE1, "{p:#x}");
        assert_eq!(w & 0x1FF, u32::from(m.tpage) & 0x1FF, "{p:#x}: tpage");
        assert_eq!(w >> 9 & 1 != 0, m.dither, "{p:#x}: dtd");
        assert_eq!(w >> 10 & 1 != 0, m.draw_on_display, "{p:#x}: dfe");
    }
    let q = PACKETS + 12;
    let w = cpu.mem.rd32(q + 4);
    assert_eq!((w >> 24) as u8, pass.quad.cmd);
    assert_eq!((w as u8, (w >> 8) as u8, (w >> 16) as u8), pass.quad.color);
    let verts: [(i16, i16); 4] = std::array::from_fn(|i| {
        let at = q + 8 + 4 * i as u32;
        (cpu.mem.rd16(at) as i16, cpu.mem.rd16(at + 2) as i16)
    });
    assert_eq!(verts, pass.quad.verts);
}
