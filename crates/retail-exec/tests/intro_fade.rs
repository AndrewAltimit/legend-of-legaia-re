//! The transition tick `FUN_801CF5BC` (PROT 0979)'s second switch - the
//! per-style screen fade - against
//! `legaia_render_kernels::battle_intro_styles::intro_fade`: every style
//! selector, both sub-styles and every clock around each ramp, the entity
//! parked past the phase switch (phase `8`) and every call outside the
//! routine stubbed but the fade emitter `FUN_80024EE4`, whose arguments are
//! recorded. The first switch's phase bodies are `tick_transition`'s and
//! not compared here.

use crate::common::{disc, scus_cpu};
use legaia_render_kernels::battle_intro_styles::{IntroStyle, intro_fade};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801C_F5BC;
const TICK_END: u32 = 0x801C_FBB4;
const FADE: u32 = 0x8002_4EE4;
const TOTAL: u32 = 0x801D_2458;
const STYLE: u32 = 0x801D_2460;
const SUB_STYLE: u32 = 0x801D_2464;
const ENTITY: u32 = 0x8012_0000;

#[test]
fn the_intro_fade_ramps_as_retail_does() {
    differential!(prot(979, TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(979).expect("PROT 0979"));
    cpu.stub_outside = Some((TICK, TICK_END));
    let fades = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let f = fades.clone();
    cpu.hook(FADE, move |r, _| {
        f.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]))
    });
    let mut ramped = 0;
    for selector in 0..5 {
        let style = IntroStyle::from_selector(selector).expect("a style");
        for sub in [0, 2] {
            for total in [0x30, 0x60] {
                for elapsed in -2..total + 0x50 {
                    cpu.mem.load(ENTITY, &[0u8; 0x40]);
                    cpu.mem.wr16(ENTITY + 0x22, 8);
                    cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
                    cpu.mem.wr32(TOTAL, total as u32);
                    cpu.mem.wr32(STYLE, selector as u32);
                    cpu.mem.wr32(SUB_STYLE, sub as u32);
                    fades.borrow_mut().clear();
                    assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned);
                    let want: Vec<(u32, u32, u32)> = intro_fade(style, sub, elapsed, total)
                        .map(|f| (u32::from(f.layer), u32::from(f.abr), f.rgb))
                        .into_iter()
                        .collect();
                    ramped += want.len();
                    assert_eq!(
                        *fades.borrow(),
                        want,
                        "style {selector} sub {sub} total {total:#x} clock {elapsed}"
                    );
                }
            }
        }
    }
    assert!(ramped > 500, "{ramped}");
}
