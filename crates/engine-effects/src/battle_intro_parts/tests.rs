use super::*;

/// Each style arm seats the init's count: scatter `3` / `7` / `11` by
/// sub-style, spin-up `2`, shatter `1`, curtain none, swirl `10`.
#[test]
fn each_style_arm_seats_the_inits_parts() {
    let n = |s, sub| style_seats(s, sub).len();
    assert_eq!(n(IntroStyle::ScatterParticles, -1), 0);
    assert_eq!(n(IntroStyle::ScatterParticles, 0), 3);
    assert_eq!(n(IntroStyle::ScatterParticles, 1), 7);
    assert_eq!(n(IntroStyle::ScatterParticles, 2), 11);
    assert_eq!(n(IntroStyle::SpinUpParticles, 0), 2);
    assert_eq!(n(IntroStyle::TileShatter, 0), 1);
    assert_eq!(n(IntroStyle::Curtain, 0), 0);
    assert_eq!(n(IntroStyle::Swirl, 0), 10);
}

/// A synthetic record in the shipped shape: an additive double-sided colour
/// word at full level, a star (shape `2`) of 8 columns, a falling level
/// rate, a wait and a halt.
fn star_record() -> Arc<[u16]> {
    let mut w: Vec<u16> = vec![
        0xFFFF, 0xFFFF, // model_sel -1
        0x0C, 0xFF89, 0, 0, 0, 0x1000, // colour word 0xC9, level 0x1000
        0x13, 0x10, 0x08, 0, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0x20, 0x100, 0, 0, 0x1000,
        0x1000, // DK4 star, 8 columns, white, radii 0x20 / 0x100
        0x0D, 0xFFC0, // level rate -512
        0x09, 0x40, // wait 64
        0x08, // halt
    ];
    w.resize(RECORD_WINDOW_WORDS, 0);
    w.into()
}

/// The part seats hidden behind its level, fades in as the level falls, and
/// draws round the screen centre the transition's GTE file projects
/// `(0, 0, 0x3C0)` to.
#[test]
fn a_seated_star_fades_in_at_the_screen_centre() {
    let mut p = IntroPart::spawn(&seat(0x801D_1F14), star_record());
    assert!(!p.halted);
    assert_eq!(p.state.move_substate, 4, "op 0x13 makes it draw kind 4");
    let mut first = Vec::new();
    p.emit(&mut first);
    assert!(!first.is_empty(), "a default-arm node draws from its seat");
    let lum = |prims: &[ScreenPrim]| -> u32 {
        prims
            .iter()
            .map(|q| match q {
                ScreenPrim::Flat(f) => f.gouraud.unwrap().iter().map(|c| u32::from(c[0])).sum(),
                ScreenPrim::Textured(_) => 0,
            })
            .sum()
    };
    assert_eq!(lum(&first), 0, "level 0x1000 cues every corner to black");
    for _ in 0..8 {
        p.tick(3 * SPEED_SCALAR);
    }
    let mut later = Vec::new();
    p.emit(&mut later);
    assert!(lum(&later) > 0, "the falling level brings the star in");
    let (mut sx, mut sy, mut n) = (0i32, 0i32, 0i32);
    for q in &later {
        for (x, y) in q.corners() {
            sx += i32::from(x);
            sy += i32::from(y);
            n += 1;
        }
    }
    let (cx, cy) = (sx / n, sy / n);
    assert!(
        (cx - INTRO_OFX).abs() <= 4 && (cy - INTRO_OFY).abs() <= 4,
        "centre {cx},{cy}"
    );
    for q in &later {
        let ScreenPrim::Flat(f) = q else { panic!() };
        assert!(f.semi_transparent);
        assert_eq!(f.abr_mode, 1, "mode byte 0xC9 adds");
    }
}

/// A halted part draws nothing (the walk's flag-`0x8` skip).
#[test]
fn a_halted_part_draws_nothing() {
    let mut p = IntroPart::spawn(&seat(0x801D_1F14), star_record());
    for _ in 0..40 {
        p.tick(3 * SPEED_SCALAR);
    }
    assert!(p.halted);
    let mut prims = Vec::new();
    p.emit(&mut prims);
    assert!(prims.is_empty());
}

/// The init's stores land after the first VM run, over what the record set.
#[test]
fn the_inits_stores_override_the_records_own() {
    let s = Seat {
        wait: Some(0x50),
        colour: Some(0x00A0_0000),
        radius: Some((0xF, 0x28)),
        rate_c0: Some(0),
        ..seat(0x801D_2000)
    };
    let p = IntroPart::spawn(&s, star_record());
    assert_eq!(p.state.wait_timer, 0x50);
    assert_eq!(p.state.actor_u32(0xA0), 0x00A0_0000);
    assert_eq!(p.state.actor_u16(0xB4), 0xF);
    assert_eq!(p.state.actor_u16(0xB6), 0x28);
}
