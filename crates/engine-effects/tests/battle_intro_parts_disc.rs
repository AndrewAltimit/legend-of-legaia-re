//! Disc-gated: the scatter transition's parts, seated out of PROT 0979 and
//! ticked on retail's frame steps, hold the state a PCSX-Redux run of the
//! same transition reads.
//!
//! The oracle is a run from the `retona_field_card_boot` film's exit state
//! (formation `[136]`, slot-0 monster `0x88`, scatter sub-style `1`): an
//! exec breakpoint on the part tick `FUN_80021DF4` logged each of the seven
//! parts' `+0x54`, `+0x78`, `+0xB4`, `+0xB6` and `+0x28` before every tick.
//! The transition's first frames run at steps `2` and `4`, then `3`. The
//! numbers below are those logs after 6, 12 and 21 ticks. No Sony bytes:
//! the records are read off the local disc.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_engine_effects::battle_intro_parts::IntroParts;
use legaia_prot::archive::Archive;
use legaia_render_kernels::battle_intro::IntroPartRuntime;
use legaia_render_kernels::battle_intro_styles::IntroStyle;
use legaia_render_kernels::screen_prim::ScreenPrim;

fn prot() -> Option<PathBuf> {
    ["extracted", "../../extracted", "../extracted"]
        .iter()
        .map(|b| PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())
}

/// `(+0x54, +0x78, +0xB4, +0xB6, +0x28)` per part, in seat order.
type Row = [(i16, u16, i16, i16, i16); 7];

const AFTER_6: Row = [
    (368, 2944, 2, 1472, 576),
    (240, 0, 2, 0, 0),
    (240, 0, 0, 4, 0),
    (1504, 192, 12, 198, 0),
    (1504, 192, 27, 208, 0),
    (1504, 192, 42, 228, 0),
    (1504, 192, 0, 124, 192),
];
const AFTER_12: Row = [
    (224, 1792, 2, 896, 1152),
    (96, 0, 2, 0, 0),
    (96, 0, 0, 4, 0),
    (1360, 480, 30, 450, 0),
    (1360, 480, 45, 460, 0),
    (1360, 480, 60, 480, 0),
    (1360, 480, 0, 304, 480),
];
const AFTER_21: Row = [
    (8, 64, 2, 32, 2016),
    (416, 384, 386, 768, 192),
    (416, 768, 384, 388, 0),
    (1144, 912, 57, 828, 0),
    (1144, 912, 72, 838, 0),
    (1144, 912, 87, 858, 0),
    (1144, 912, 0, 574, 912),
];

fn row(parts: &IntroParts) -> Row {
    std::array::from_fn(|i| {
        let s = &parts.parts()[i].state;
        (
            s.wait_timer,
            s.field_78,
            s.actor_u16(0xB4) as i16,
            s.actor_u16(0xB6) as i16,
            s.render_28,
        )
    })
}

#[test]
fn scatter_parts_follow_retails_ticks() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive.entries[979].clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read 0979");
    let mut parts =
        IntroParts::seat(IntroStyle::ScatterParticles, 1, &bytes).expect("seat the scatter parts");
    assert_eq!(parts.parts().len(), 7);
    let steps = [2u8, 4, 3, 3, 3, 3];
    for s in steps {
        parts.step(s);
    }
    assert_eq!(row(&parts), AFTER_6, "after six ticks");
    for _ in 0..5 {
        parts.step(3);
    }
    // The frame the film's ordering table holds 42 vsyncs past the exit
    // state: 200 `GT4` packets (`0x3E`), centred on `(160, 114)`, the 8-ray
    // star's tips 131..133 pixels out and the three tinted rings' 53..59.
    let mut prims = Vec::new();
    parts.emit(&mut prims);
    assert_eq!(prims.len(), 200, "the starburst's packet count");
    let reach = |q: &ScreenPrim| {
        q.corners()
            .iter()
            .map(|&(x, y)| f64::from(i32::from(x) - 160).hypot(f64::from(i32::from(y) - 114)))
            .fold(0.0, f64::max)
    };
    let far = prims.iter().map(reach).fold(0.0, f64::max);
    assert!((131.0..=133.5).contains(&far), "ray tips {far}");
    for q in &prims {
        let ScreenPrim::Flat(f) = q else {
            panic!("the parts draw flat gouraud quads")
        };
        assert!(f.semi_transparent && f.abr_mode == 1);
        let tinted = f.gouraud.unwrap().iter().any(|c| {
            let lit = c[..3].iter().filter(|&&v| v > 0).count();
            lit == 1
        });
        if tinted {
            assert!((53.0..=59.5).contains(&reach(q)), "ring {}", reach(q));
        }
    }
    parts.step(3);
    assert_eq!(row(&parts), AFTER_12, "after twelve ticks");
    for _ in 0..9 {
        parts.step(3);
    }
    assert_eq!(row(&parts), AFTER_21, "after twenty-one ticks");
}
