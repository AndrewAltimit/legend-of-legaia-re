//! The curtain transition style's frame `FUN_801D11D0` (PROT 0979) against
//! `legaia_render_kernels::battle_intro_styles::tick_curtain`: every clock
//! value the style runs through, with the quad builder `FUN_801CF1B0`
//! hooked to record each request - anchor, pen, key, intensity, both
//! scales and the OT depth staged in `DAT_801D245C` - and every other call
//! outside the routine stubbed. The requests, in order, the descriptor
//! fields patched before each, and the clock advance match the port's.

use crate::common::{disc, scus_cpu};
use legaia_render_kernels::battle_intro::IntroQuadTable;
use legaia_render_kernels::battle_intro_styles::tick_curtain;
use legaia_render_kernels::battle_intro_transition::{IntroQuadAnchor, IntroQuadRequest};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CURTAIN: u32 = 0x801D_11D0;
const CURTAIN_END: u32 = 0x801D_1564;
const BUILDER: u32 = 0x801C_F1B0;
const OT_DEPTH: u32 = 0x801D_245C;
const TABLE: u32 = 0x801D_1EC4;
const FRAME_STEP: u32 = 0x1F80_0393;
const ENTITY: u32 = 0x8012_0000;

#[test]
fn the_curtain_frame_requests_what_tick_curtain_requests() {
    differential!(prot(979, CURTAIN));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    let image = d.read_entry(979).expect("PROT 0979");
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.stub_outside = Some((CURTAIN, CURTAIN_END));
    let log = Rc::new(RefCell::new(Vec::<(IntroQuadRequest, [u8; 0x14])>::new()));
    let l = log.clone();
    cpu.hook(BUILDER, move |r, m| {
        let sp = r.r[SP];
        let key = r.r[A3] as i32;
        let desc = TABLE + 0x14 * (key as u32 & 0x3FF);
        let bytes: [u8; 0x14] = std::array::from_fn(|i| m.rd8(desc + i as u32));
        l.borrow_mut().push((
            IntroQuadRequest {
                anchor: if r.r[A0] == 0 {
                    IntroQuadAnchor::Centre
                } else {
                    IntroQuadAnchor::TopLeft
                },
                x: r.r[A1] as i16,
                y: r.r[A2] as i16,
                key,
                intensity: m.rd32(sp + 0x10) as i32,
                scale_x: m.rd32(sp + 0x14) as i32,
                scale_y: m.rd32(sp + 0x18) as i32,
                ot_depth: m.rd32(OT_DEPTH),
            },
            bytes,
        ));
    });
    let mut port_table = IntroQuadTable::parse_overlay(&image, OVERLAY_BASE)
        .expect("the descriptor table")
        .0;
    let mut culled = 0;
    for (case, elapsed) in (0..0x60i16).chain([0x100, 0x7FF0]).enumerate() {
        let step = 1 + (case % 3) as u8;
        cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
        cpu.mem.wr8(FRAME_STEP, step);
        log.borrow_mut().clear();
        assert_eq!(
            cpu.call(CURTAIN, &[ENTITY]),
            Stop::Returned,
            "clock {elapsed}"
        );
        let mut clock = elapsed;
        let tick = tick_curtain(&mut port_table, &mut clock, step);
        assert_eq!(
            cpu.mem.rd16(ENTITY + 0x1A) as i16,
            clock,
            "clock {elapsed}: the advance"
        );
        let got = log.borrow();
        assert_eq!(
            got.len(),
            tick.quads.len(),
            "clock {elapsed}: the request count"
        );
        for (k, ((req, desc), q)) in got.iter().zip(&tick.quads).enumerate() {
            assert_eq!(*req, q.request, "clock {elapsed} request {k}");
            // The descriptor as retail patched it is the one the port built
            // from: its quad's texels and page.
            let port = legaia_render_kernels::battle_intro_transition::IntroQuadDesc::parse(desc)
                .expect("a descriptor");
            assert_eq!(
                legaia_render_kernels::battle_intro_transition::build_intro_quad(&q.request, &{
                    let mut t = port_table.clone();
                    t[q.desc_index] = port;
                    t
                }),
                Some(q.quad),
                "clock {elapsed} request {k}: the patched descriptor"
            );
        }
        culled += tick.culled_columns;
    }
    assert!(culled > 1000, "{culled}");
}
