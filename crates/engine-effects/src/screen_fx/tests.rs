use super::*;

// -- interp (FUN_801DE4C8) ---------------------------------------------

#[test]
fn interp_linear_matches_mode1() {
    assert_eq!(interp(100, 0, 0, 10, InterpMode::Linear), 0);
    assert_eq!(interp(100, 0, 5, 10, InterpMode::Linear), 50);
    assert_eq!(interp(100, 0, 10, 10, InterpMode::Linear), 100);
    assert_eq!(
        interp(100, 0, 11, 10, InterpMode::Linear),
        100,
        "t>D clamps"
    );
    assert_eq!(interp(50, 50, 3, 10, InterpMode::Linear), 50, "a==b early");
    // Truncation toward zero both directions.
    assert_eq!(interp(10, 0, 3, 10, InterpMode::Linear), 3);
    assert_eq!(interp(0, 10, 3, 10, InterpMode::Linear), 7);
}

#[test]
fn interp_ease_out_mode2() {
    // e = (a-b)*t; result = (e + (e/D)*(D-t))/D + b.
    // a=100,b=0,t=5,D=10: e=500, e/D=50, +50*5=750, /10=75.
    assert_eq!(interp(100, 0, 5, 10, InterpMode::EaseOut), 75);
    // Starts fast: t=1 -> e=100, e/D=10, +10*9=190, /10=19.
    assert_eq!(interp(100, 0, 1, 10, InterpMode::EaseOut), 19);
    assert_eq!(interp(100, 0, 10, 10, InterpMode::EaseOut), 100);
}

#[test]
fn interp_ease_in_mode3() {
    // ((a-b)*t/D)*t/D + b: t=5,D=10 -> (100*5/10)*5/10 = 25.
    assert_eq!(interp(100, 0, 5, 10, InterpMode::EaseIn), 25);
    assert_eq!(interp(100, 0, 1, 10, InterpMode::EaseIn), 1);
    assert_eq!(interp(100, 0, 10, 10, InterpMode::EaseIn), 100);
}

#[test]
fn interp_ease_in_out_mode4() {
    // First half = ease-in over D/2 covering half the span:
    // t=2,D=20 (half=10, span=50): (50*2/10)*2/10 = 2.
    assert_eq!(interp(100, 0, 2, 20, InterpMode::EaseInOut), 2);
    // Exactly half: (50*10/10)*10/10 = 50.
    assert_eq!(interp(100, 0, 10, 20, InterpMode::EaseInOut), 50);
    // Second half = ease-out from the midpoint: t=15 -> mode-2 with
    // a=100,b=50,t=5,D=10: e=250, e/D=25, +25*5=375, /10=37 -> 87.
    assert_eq!(interp(100, 0, 15, 20, InterpMode::EaseInOut), 87);
    assert_eq!(interp(100, 0, 20, 20, InterpMode::EaseInOut), 100);
}

/// The image panel's operands are buffer rows: on the field's screen
/// the quad sits the draw-area offset above its script Y
/// (`0x801F86C0..0x801F86E4`).
#[test]
fn the_panel_draws_the_draw_area_offset_above_its_script_row() {
    let mut rec = vec![0u8; 0xd];
    rec[1..3].copy_from_slice(&16i16.to_le_bytes());
    rec[3..5].copy_from_slice(&100i16.to_le_bytes());
    rec[5..7].copy_from_slice(&140i16.to_le_bytes());
    rec[7..9].copy_from_slice(&82i16.to_le_bytes());
    let mut p = PanelWidget::spawn(&rec).unwrap();
    let (q, second) = p.tick(1, &Screen::default());
    assert!(second.is_none());
    assert_eq!((q.left, q.top, q.right, q.bottom), (16, 96, 155, 177));
    // A caller that hands a zero offset gets the script row back.
    let (q, _) = p.tick(1, &screen());
    assert_eq!(q.top, 100);
}

// -- mask widget (FUN_801F811C / FUN_801F8D4C) -------------------------

fn screen() -> Screen {
    Screen {
        x0: 0,
        y_off: 0,
        height: 240,
    }
}

#[test]
fn mask_spawns_fully_open() {
    let s = screen();
    let mut m = MaskWidget::spawn(&s);
    assert_eq!(m.cur, [0, 0, 0x140, 239]);
    let q = m.tick(10, &s);
    // All four border quads are degenerate or inverted (cover nothing).
    assert_eq!(q[0].bottom, 0, "top band has zero height");
    assert_eq!(q[1].top, 239, "bottom band starts at the last row");
    assert_eq!(q[2].right, -1, "left band right edge = L-1 < L");
    assert_eq!(q[3].left, 0x140, "right band starts at the right edge");
}

#[test]
fn mask_set_rect_substitutes_defaults_for_minus_one() {
    let s = Screen {
        x0: 8,
        y_off: 0,
        height: 224,
    };
    let mut m = MaskWidget::spawn(&s);
    m.set_rect(-1, -1, -1, -1, 16, &s);
    assert_eq!(m.target, [8, 0, 0x140, 223]);
    m.set_rect(100, 50, 220, 190, 16, &s);
    assert_eq!(m.target, [100, 50, 220, 190]);
    assert_eq!((m.t, m.dur), (0, 16));
}

#[test]
fn mask_tick_reinterpolates_from_fixed_start_and_latches() {
    let s = screen();
    let mut m = MaskWidget::spawn(&s);
    m.set_rect(100, 60, 240, 180, 40, &s);

    // Retail re-lerps from the latched start each frame: at t=10 of 40,
    // top = 0 + (60-0)*10/40 = 15 (NOT an iterative step).
    let q = m.tick(10, &s);
    assert_eq!(q[0].bottom, 15, "top edge at t=10");
    assert_eq!(m.cur[1], 0, "cur does not move mid-tween");

    let q = m.tick(10, &s);
    assert_eq!(q[0].bottom, 30, "top edge at t=20 from the same start");
    let q = m.tick(10, &s);
    assert_eq!(q[0].bottom, 45);

    // t reaches dur: latch, dur clears, edges exact.
    let q = m.tick(10, &s);
    assert_eq!(m.cur, [100, 60, 240, 180]);
    assert_eq!(m.dur, 0);
    assert_eq!(
        q,
        [
            MaskQuad {
                left: 0,
                top: 0,
                right: 0x140,
                bottom: 60
            },
            MaskQuad {
                left: 0,
                top: 180,
                right: 0x140,
                bottom: 239
            },
            MaskQuad {
                left: 0,
                top: 60,
                right: 99,
                bottom: 180
            },
            MaskQuad {
                left: 240,
                top: 60,
                right: 0x140,
                bottom: 180
            },
        ]
    );

    // Post-latch frames take the dur==0 snap arm and hold.
    let q2 = m.tick(10, &s);
    assert_eq!(q2, q);
}

#[test]
fn mask_quads_frame_the_visible_rect() {
    let s = screen();
    let mut m = MaskWidget::spawn(&s);
    m.set_rect(80, 40, 260, 200, 0, &s);
    // dur == 0 -> snap arm.
    let q = m.tick(1, &s);
    // The union of the four bands covers everything outside [80..260]x[40..200].
    assert_eq!(
        q[0],
        MaskQuad {
            left: 0,
            top: 0,
            right: 0x140,
            bottom: 40
        }
    );
    assert_eq!(
        q[1],
        MaskQuad {
            left: 0,
            top: 200,
            right: 0x140,
            bottom: 239
        }
    );
    assert_eq!(
        q[2],
        MaskQuad {
            left: 0,
            top: 40,
            right: 79,
            bottom: 200
        }
    );
    assert_eq!(
        q[3],
        MaskQuad {
            left: 260,
            top: 40,
            right: 0x140,
            bottom: 200
        }
    );
}

// -- sprite widget (FUN_801F8004 / FUN_801F7A9C) -----------------------

/// Build a sprite spawn record: header + script bytes.
fn sprite_record(script: &[u8]) -> Vec<u8> {
    let mut r = Vec::new();
    for v in [50i16, 60, 32, 16] {
        r.extend_from_slice(&v.to_le_bytes()); // x y w h
    }
    for v in [0x143i16, 0x1A5, 0x320, 0x1E2] {
        r.extend_from_slice(&v.to_le_bytes()); // tex_x tex_y clut_x clut_y
    }
    r.extend_from_slice(&[0x80, 0x40, 0x20]); // rgb
    assert_eq!(r.len(), SPRITE_SCRIPT_OFF);
    r.extend_from_slice(script);
    r
}

#[test]
fn sprite_record_field_derivation() {
    let rec = sprite_record(&[]);
    let p = SpriteRecord::parse(&rec).unwrap();
    assert_eq!((p.x, p.y, p.w, p.h), (50, 60, 32, 16));
    // texpage = (0x143 >> 6) + ((0x1A5 & !0xff) >> 4) = 5 + 0x10 = 0x15.
    assert_eq!(p.texpage, 0x15);
    // u = (0x143 & 0x3f) << 2 = 3 << 2 = 12 ; v = 0x1A5 & 0xff = 0xA5.
    assert_eq!((p.u, p.v), (12, 0xA5));
    // clut = (0x1E2 << 6) + (0x320 >> 4) = 0x7880 + 0x32 = 0x78B2.
    assert_eq!(p.clut as u16, 0x78B2);
    assert_eq!(p.rgb, [0x80, 0x40, 0x20]);
}

/// The ending credits' shape: the spawn payload runs past the record into
/// the script that follows it, so the widget fades in from the record's
/// black, holds while its flag is up, fades out and dies.
#[test]
fn a_spawned_sprite_runs_the_script_after_its_record() {
    let mut rec = sprite_record(&[
        0x40, 0x07, 0x04, 0x80, 0x80, 0x80, 0x01, 0x30, 0x00, // tween to 0x808080 over 0x30
        0x40, 0x03, 0x02, 0x0B, 0x00, // wait for flag 0x0B clear
        0x40, 0x07, 0x04, 0x00, 0x00, 0x00, 0x01, 0x50, 0x00, // fade out over 0x50
        0x40, 0x01, 0x00, // kill
    ]);
    rec[0x10..0x13].copy_from_slice(&[0, 0, 0]);
    let mut host = ScreenFxHost::default();
    host.sprite_spawn(&rec);
    let mut flag = true;
    let mut peak = 0u8;
    for _ in 0..0x30 {
        let f = host.tick(1, |_| flag);
        peak = peak.max(f.sprites[0].rgb[0]);
    }
    assert_eq!(peak, 0x80, "faded in to the neutral modulation");
    let held = host.tick(1, |_| flag);
    assert_eq!(held.sprites[0].rgb, [0x80; 3], "holds while the flag is up");
    flag = false;
    let mut frames = 0;
    while !host.tick(1, |_| flag).sprites.is_empty() {
        frames += 1;
        assert!(frames < 0x60, "fade-out ends in the kill");
    }
}

#[test]
fn sprite_subop_0_kills_the_draw() {
    let rec = sprite_record(&[0x40, 0, 0]);
    let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
    assert!(w.tick(&rec, 16, |_| false).is_none());
    assert!(w.killed);
    assert!(w.tick(&rec, 16, |_| false).is_none(), "stays dead");
}

#[test]
fn sprite_subops_1_2_gate_on_story_flags() {
    // wait-until-set(flag 7) ; kill.
    let rec = sprite_record(&[0x40, 0, 1, 7, 0, 0x40, 0, 0]);
    let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
    // Flag clear: parked, still draws.
    let d = w.tick(&rec, 16, |_| false).unwrap();
    assert_eq!((d.x, d.y), (50, 60));
    assert_eq!(w.cursor, SPRITE_SCRIPT_OFF, "cursor parked on the wait");
    // Flag set: advances AND interprets the next op (kill) the same frame.
    assert!(w.tick(&rec, 16, |f| f == 7).is_none());

    // wait-until-clear: inverted gate.
    let rec2 = sprite_record(&[0x40, 0, 2, 7, 0, 0x40, 0, 0]);
    let mut w2 = SpriteWidget::spawn(SpriteRecord::parse(&rec2).unwrap());
    assert!(w2.tick(&rec2, 16, |_| true).is_some(), "set -> parked");
    assert!(w2.tick(&rec2, 16, |_| false).is_none(), "clear -> advance");
}

#[test]
fn sprite_subop_3_tweens_position_and_colour_from_captured_start() {
    // Tween to (90, 100), colour (0,0,0), linear, over 40.
    let mut script = vec![0x40, 0, 3];
    script.extend_from_slice(&90i16.to_le_bytes());
    script.extend_from_slice(&100i16.to_le_bytes());
    script.extend_from_slice(&[0, 0, 0]); // target rgb
    script.push(1); // mode = linear
    script.extend_from_slice(&40i16.to_le_bytes());
    let rec = sprite_record(&script);
    let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());

    let d = w.tick(&rec, 10, |_| false).unwrap();
    // x: 50 + (90-50)*10/40 = 60 ; y: 60 + (100-60)*10/40 = 70.
    assert_eq!((d.x, d.y), (60, 70));
    // r: 0x80 + (0-0x80)*10/40 = 0x60.
    assert_eq!(d.rgb, [0x60, 0x30, 0x18]);

    let d = w.tick(&rec, 10, |_| false).unwrap();
    assert_eq!((d.x, d.y), (70, 80), "re-lerped from the captured start");

    w.tick(&rec, 10, |_| false);
    let d = w.tick(&rec, 10, |_| false).unwrap();
    assert_eq!((d.x, d.y), (90, 100), "exact landing");
    assert_eq!(d.rgb, [0, 0, 0]);
    assert_eq!(w.t, 0, "clock reset for the next op");
    assert_eq!(w.cursor, SPRITE_SCRIPT_OFF + 0xd, "cursor advanced");
}

#[test]
fn sprite_subop_4_tweens_colour_only() {
    let mut script = vec![0x40, 0, 4];
    script.extend_from_slice(&[0xFF, 0xFF, 0xFF]); // target rgb
    script.push(1); // mode
    script.extend_from_slice(&16i16.to_le_bytes());
    let rec = sprite_record(&script);
    let mut w = SpriteWidget::spawn(SpriteRecord::parse(&rec).unwrap());
    let d = w.tick(&rec, 8, |_| false).unwrap();
    assert_eq!((d.x, d.y), (50, 60), "position untouched");
    // r: 0x80 + (0xFF-0x80)*8/16 = 0x80 + 0x3F = 0xBF.
    assert_eq!(d.rgb[0], 0xBF);
    let d = w.tick(&rec, 8, |_| false).unwrap();
    assert_eq!(d.rgb, [0xFF, 0xFF, 0xFF]);
    assert_eq!(w.cursor, SPRITE_SCRIPT_OFF + 9);
}

// -- panel widget (FUN_801F88FC / FUN_801F8E6C / FUN_801F849C) ---------

/// A panel spawn record (field-VM operand shape: data from byte 1).
fn panel_record(w: i16) -> Vec<u8> {
    let mut r = vec![0u8]; // operand byte 0 (the opcode-adjacent byte)
    for v in [40i16, 30, w, 100, 0x140, 0x100] {
        r.extend_from_slice(&v.to_le_bytes());
    }
    r
}

#[test]
fn panel_spawn_single_page() {
    let p = PanelWidget::spawn(&panel_record(0x100)).unwrap();
    assert_eq!(p.cur, [40, 30, 0x100, 100, 0x100]);
    // u = (0x140 & 0x3f) << 2 = 0 ; page = (0x140>>6) + ((0x100 & 0xff80)>>4) | 0x100.
    assert_eq!(p.u, 0);
    assert_eq!(p.texpage, (5 + 0x10) | 0x100);
    assert_eq!(p.texpage2, 0, "<=256 wide: one page");
    assert_eq!(p.v, 0, "v = tex_y & 0x7f");
}

#[test]
fn panel_spawn_wide_splits_two_pages() {
    let p = PanelWidget::spawn(&panel_record(0x140)).unwrap();
    assert_eq!(p.cur[4], 0x100, "first-page width clamps to 256");
    assert_eq!(p.base, [0x140, 100, 0x100]);
    // texpage2 = ((0x140 + 0x100) >> 6) + 0x10 | 0x100 = (9 + 0x10) | 0x100.
    assert_eq!(p.texpage2, (9 + 0x10) | 0x100);
    let s = screen();
    let mut p2 = p;
    let (q0, q1) = p2.tick(1, &s);
    assert_eq!((q0.left, q0.right), (40, 40 + 0x100 - 1));
    let q1 = q1.expect("second page quad");
    assert_eq!((q1.left, q1.right), (40 + 0x100 - 2, 40 + 0x140));
    assert_eq!(q1.texpage, (9 + 0x10) | 0x100);
}

#[test]
fn panel_move_scale_tweens_five_channels() {
    let mut p = PanelWidget::spawn(&panel_record(0x100)).unwrap();
    // Scale to half size while moving to (100, 80) over 20 frames.
    p.move_scale(100, 80, 0x800, 20);
    assert_eq!(p.target, [100, 80, 0x80, 50, 0x80]);
    let s = screen();
    let (q0, _) = p.tick(10, &s);
    // x: 40 + (100-40)*10/20 = 70 ; w0 channel: 0x100 + (0x80-0x100)*10/20 = 0xC0.
    assert_eq!(q0.left, 70);
    assert_eq!(q0.right, 70 + 0xC0 - 1);
    let _ = p.tick(10, &s);
    assert_eq!(p.cur, [100, 80, 0x80, 50, 0x80], "latched");
    assert_eq!(p.dur, 0);
}

// -- letterbox widget (FUN_801F8F28 / FUN_801F8A34) --------------------

#[test]
fn letterbox_bands_and_feathers() {
    let mut block = Vec::new();
    for v in [0i16, 0x140, 40, 56, 184, 200] {
        block.extend_from_slice(&v.to_le_bytes());
    }
    let lb = Letterbox::from_config(&block).unwrap();
    let s = screen();
    let solid = lb.solid_bands(&s);
    assert_eq!(
        solid[0],
        MaskQuad {
            left: 0,
            top: 0,
            right: 0x140,
            bottom: 40
        }
    );
    assert_eq!(
        solid[1],
        MaskQuad {
            left: 0,
            top: 200,
            right: 0x140,
            bottom: 240
        },
        "bottom band ends at height (retail literal, not height-1)"
    );
    let grad = lb.gradient_bands();
    assert_eq!((grad[0].0.top, grad[0].0.bottom, grad[0].1), (40, 56, true));
    assert_eq!(
        (grad[1].0.top, grad[1].0.bottom, grad[1].1),
        (184, 200, false)
    );
}
