use super::*;

#[test]
fn the_selector_covers_exactly_the_five_ported_style_bodies() {
    // Both switches bound with `sltiu v0,v1,0x5`, and both jump tables
    // carry five words - checked against the disc image, not against
    // Ghidra's switch recovery.
    assert_eq!(INTRO_FADE_RAMPS.len(), INTRO_STYLE_COUNT as usize);
    assert_eq!(IntroStyle::from_selector(-1), None);
    assert_eq!(IntroStyle::from_selector(5), None);
    let addrs: Vec<u32> = (0..INTRO_STYLE_COUNT)
        .map(|i| IntroStyle::from_selector(i).unwrap().tick_address())
        .collect();
    assert_eq!(
        addrs,
        vec![
            0x801C_FDA0,
            0x801D_0370,
            0x801D_0D24,
            0x801D_11D0,
            0x801D_1888
        ],
        "the dispatch order is the jump table's, not the enum's"
    );
}

fn pick(flags: u8, slot0: u8, scene: u32) -> IntroStyleChoice {
    select_intro_style(&IntroStyleInputs {
        battle_flags: flags,
        formation_slot0: slot0,
        scene_index: scene,
    })
}

#[test]
fn the_ordinary_encounter_gets_the_tile_shatter() {
    // The default, and the arm an unflagged battle takes.
    assert_eq!(
        pick(0, 1, 0),
        IntroStyleChoice {
            style: IntroStyle::TileShatter,
            sub_style: 0
        }
    );
}

#[test]
fn the_flag_bit_arms_the_spin_up_field_except_over_one_id_window() {
    // Retail stores selector 1 in a branch delay slot, so it lands on both
    // paths and only the in-range arm overwrites it.
    for slot0 in [0x00u8, 0x12, 0x16, 0xFE] {
        assert_eq!(pick(0x80, slot0, 0).style, IntroStyle::SpinUpParticles);
    }
    for slot0 in 0x13u8..=0x15 {
        assert_eq!(
            pick(0x80, slot0, 0),
            IntroStyleChoice {
                style: IntroStyle::TileShatter,
                sub_style: 1
            }
        );
    }
    // Only bit 0x80 is read.
    assert_eq!(pick(0x7F, 0, 0).style, IntroStyle::TileShatter);
}

#[test]
fn three_formations_take_the_curtain() {
    for slot0 in [0x4Fu8, 0xAF, 0xA5] {
        assert_eq!(pick(0, slot0, 0).style, IntroStyle::Curtain);
    }
    assert_eq!(pick(0, 0x50, 0).style, IntroStyle::TileShatter);
}

#[test]
fn the_scatter_overrides_carry_their_own_sub_style() {
    for (slot0, sub) in [(0x4Cu8, 0), (0x88, 1), (0xB3, 2)] {
        assert_eq!(
            pick(0, slot0, 0),
            IntroStyleChoice {
                style: IntroStyle::ScatterParticles,
                sub_style: sub
            }
        );
    }
}

#[test]
fn two_overrides_are_scene_conditional() {
    for slot0 in [0x3Eu8, 0x3F] {
        for scene in [3u32, 0x0C, 0x15] {
            assert_eq!(
                pick(0, slot0, scene),
                IntroStyleChoice {
                    style: IntroStyle::TileShatter,
                    sub_style: 2
                }
            );
        }
        // The same formation in any other scene keeps the default.
        assert_eq!(pick(0, slot0, 4).sub_style, 0);
    }
    assert_eq!(pick(0, 0x80, 0x9B).sub_style, 2);
    assert_eq!(pick(0, 0x80, 0x9C).sub_style, 0);
}

#[test]
fn the_last_override_wins_and_carries_the_sub_style_before_it() {
    // 0xB5 sets only the selector, so whatever sub-style an earlier arm
    // left is carried into the swirl - that is retail leaving
    // `DAT_801D2464` alone, not an omission.
    let c = pick(0x80, 0xB5, 0);
    assert_eq!(c.style, IntroStyle::Swirl);
    assert_eq!(c.sub_style, 0);
}

#[test]
fn the_fade_does_not_start_until_the_clock_passes_the_lead() {
    // Style 3's lead is 0x40: the ramp is inactive at total - 0x40 and
    // starts one frame later. Retail's gate is `total - lead < elapsed`,
    // so the boundary frame emits nothing at all rather than a level-zero
    // quad.
    let total = 200;
    assert_eq!(
        intro_fade(IntroStyle::Curtain, 0, total - 0x40, total),
        None
    );
    let f = intro_fade(IntroStyle::Curtain, 0, total - 0x40 + 1, total).unwrap();
    assert_eq!(f.level, 4, "one frame past the lead, slope 4");
    assert_eq!(f.layer, INTRO_FADE_LAYER);
    assert_eq!(f.abr, 2, "subtractive - the curtain fades to black");
}

#[test]
fn the_level_smears_into_all_three_channels() {
    let total = 100;
    // Style 1, slope 16, lead 0x18: five frames past -> 80.
    let f = intro_fade(IntroStyle::SpinUpParticles, 0, total - 0x18 + 5, total).unwrap();
    assert_eq!(f.level, 80);
    assert_eq!(f.rgb, 0x0050_5050);
    assert_eq!(f.abr, 1, "additive - the spin-up whites out");
}

#[test]
fn the_ramp_saturates_at_full_white() {
    let total = 100;
    // Far past the threshold the product runs past 0xFF and clamps.
    let f = intro_fade(IntroStyle::TileShatter, 0, total + 100, total).unwrap();
    assert_eq!(f.level, 0xFF);
    assert_eq!(f.rgb, 0x00FF_FFFF);
}

#[test]
fn only_the_scatter_style_reads_the_sub_style_and_only_for_its_blend() {
    let total = 100;
    let at = total - 0x18 + 2;
    let plain = intro_fade(IntroStyle::ScatterParticles, 0, at, total).unwrap();
    let sub2 = intro_fade(IntroStyle::ScatterParticles, 2, at, total).unwrap();
    assert_eq!(plain.abr, 2);
    assert_eq!(sub2.abr, 1, "DAT_801D2464 == 2 takes the other arm");
    assert_eq!(plain.level, sub2.level, "the ramp itself is unaffected");
    // No other style branches on it.
    for style in [
        IntroStyle::SpinUpParticles,
        IntroStyle::TileShatter,
        IntroStyle::Curtain,
        IntroStyle::Swirl,
    ] {
        let a = intro_fade(style, 0, total - 1, total);
        let b = intro_fade(style, 2, total - 1, total);
        assert_eq!(a, b, "{style:?} must not read the sub-style");
    }
}

/// The swirl is the only arm that overrides `DAT_801D2458`.
#[test]
fn only_the_swirl_lengthens_the_intro() {
    for style in [
        IntroStyle::ScatterParticles,
        IntroStyle::SpinUpParticles,
        IntroStyle::TileShatter,
        IntroStyle::Curtain,
    ] {
        assert_eq!(intro_duration_frames(style), INTRO_DURATION_FRAMES);
    }
    assert_eq!(
        intro_duration_frames(IntroStyle::Swirl),
        INTRO_DURATION_FRAMES_SWIRL
    );
    assert_eq!(
        (INTRO_DURATION_FRAMES, INTRO_DURATION_FRAMES_SWIRL),
        (132, 252)
    );
}

/// Every fade ramp is expressed as a lead *before* the intro ends, so a
/// duration shorter than the longest lead would start the whole style
/// already fading. `0x84` clears all five with room to spare.
#[test]
fn the_retail_duration_outlasts_every_fade_lead() {
    for (i, r) in INTRO_FADE_RAMPS.iter().enumerate() {
        let style = IntroStyle::from_selector(i as i32).unwrap();
        let total = intro_duration_frames(style);
        // Every ramp starts no earlier than the transition's midpoint -
        // the curtain's `0x40` is the longest, and `0x84` still clears it.
        assert!(
            r.lead * 2 <= total,
            "{style:?}: lead {:#x} against duration {total:#x}",
            r.lead
        );
        // The ramp is inactive on the first frame and saturated by the
        // last, which is what makes the fade read as a fade.
        assert!(intro_fade(style, 0, 1, total).is_none());
        assert_eq!(
            intro_fade(style, 0, total, total).map(|f| f.level),
            Some(0xFF)
        );
    }
}

/// The tile shatter's spawn window has to fit inside the intro, or part of
/// the grid never leaves its rest pose. `rand() % 5000` against
/// `elapsed * TILE_DELAY_SCALE` puts the last record's start at frame 83.
#[test]
fn the_tile_spawn_window_fits_inside_the_intro() {
    let scale = crate::battle_intro_tiles::TILE_DELAY_SCALE;
    let last_start = 4999 / scale + 1;
    assert_eq!(last_start, 84);
    assert!(
        last_start < intro_duration_frames(IntroStyle::TileShatter),
        "every tile must get past its delay gate inside the transition"
    );
}

fn particle() -> IntroParticle {
    IntroParticle {
        tint: 0x0080_8080,
        trans: (0, 0, 0),
        rot: (0x100, 0x200, 0x300),
        spin: (0x10, 0x20, 0x40),
        delay: 0,
        trans_vel: (0x40, 0x40, 0x20),
        texel_page: 0x0080,
        texel_v: 4,
        field_16: 0,
    }
}

#[test]
fn the_ticks_visit_fewer_records_than_the_seeders_fill() {
    assert_eq!(PARTICLE_TICK_COUNT, 0x488);
    assert_eq!(
        crate::battle_intro_particles::PARTICLE_COUNT - PARTICLE_TICK_COUNT,
        120,
        "the seeders fill 1280 and the ticks visit 1160"
    );
}

#[test]
fn a_masked_particle_is_skipped_before_anything_moves() {
    let mut p = IntroParticle {
        tint: 0x0100_0000,
        ..particle()
    };
    let before = p;
    assert_eq!(
        step_particle(&mut p, &PARTICLE_TICK_A, 4, i32::MAX),
        ParticleStep::Masked
    );
    assert_eq!(p, before);
}

#[test]
fn the_delay_gate_holds_a_particle_but_still_draws_it() {
    let mut p = IntroParticle {
        delay: 100,
        ..particle()
    };
    let before = p;
    let step = step_particle(&mut p, &PARTICLE_TICK_A, 1, 0);
    assert!(matches!(step, ParticleStep::Live { moved: false, .. }));
    assert_eq!(p, before, "nothing integrated");
}

#[test]
fn style_a_integrates_by_one_and_stamps_field_16() {
    let mut p = particle();
    let step = step_particle(&mut p, &PARTICLE_TICK_A, 2, i32::MAX);
    assert!(matches!(step, ParticleStep::Live { moved: true, .. }));
    // rot += (spin * 2) >> 1 == spin
    assert_eq!(p.rot, (0x110, 0x220, 0x340));
    // trans += (trans_vel * 2) >> 1 == trans_vel
    assert_eq!(p.trans, (0x40, 0x40, 0x20));
    assert_eq!(p.field_16, 1);
    assert_eq!(p.tint, 0x0080_8080, "style A leaves the colour alone");
    assert_eq!(p.spin, (0x10, 0x20, 0x40), "and does not ramp the spin");
}

#[test]
fn style_b_ramps_the_spin_and_decays_the_colour() {
    let mut p = particle();
    step_particle(&mut p, &PARTICLE_TICK_B, 1, i32::MAX);
    // 0x10 + (0x10 >> 3) + (0x10 >> 2) == 0x16
    assert_eq!(p.spin.0, 0x16);
    assert_eq!(
        p.tint,
        0x0080_8080u32.wrapping_add(PARTICLE_COLOUR_DECAY as u32)
    );
    assert_eq!(p.field_16, 0, "style B never stamps +0x16");
}

#[test]
fn style_b_pre_divides_the_rotation_vector_it_hands_the_matrix() {
    let mut p = IntroParticle {
        spin: (0, 0, 0),
        rot: (0x800, 0x400, 0x200),
        ..particle()
    };
    let ParticleStep::Live { rot, .. } = step_particle(&mut p, &PARTICLE_TICK_B, 1, 0) else {
        panic!()
    };
    assert_eq!(rot, (0x100, 0x80, 0x40));
    assert_eq!(
        p.rot,
        (0x800, 0x400, 0x200),
        "the record itself is unscaled"
    );
}

#[test]
fn the_texel_and_page_come_out_of_one_halfword() {
    let mut p = IntroParticle {
        texel_page: 0x0C25,
        texel_v: 0x30,
        ..particle()
    };
    let ParticleStep::Live { tpage, texel, .. } = step_particle(&mut p, &PARTICLE_TICK_A, 1, 0)
    else {
        panic!()
    };
    assert_eq!(tpage, (0x0C25 >> 6) + PARTICLE_TPAGE_BIAS);
    assert_eq!(texel, (0x25, 0x30));
}

#[test]
fn only_style_a_bounds_the_depth_above() {
    assert!(particle_quad_accepted(&PARTICLE_TICK_A, 0x81, (0, 0)));
    assert!(!particle_quad_accepted(&PARTICLE_TICK_A, 0x80, (0, 0)));
    assert!(!particle_quad_accepted(&PARTICLE_TICK_A, 0x3FDC, (0, 0)));
    assert!(particle_quad_accepted(&PARTICLE_TICK_B, 0x3FDC, (0, 0)));
    assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x80, (0, 0)));
}

#[test]
fn the_screen_window_is_exclusive_at_both_ends() {
    assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (-8, 0)));
    assert!(particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (-7, 0)));
    assert!(!particle_quad_accepted(&PARTICLE_TICK_B, 0x100, (0, 0xF8)));
    assert!(particle_quad_accepted(
        &PARTICLE_TICK_B,
        0x100,
        (0x147, 0xF7)
    ));
}

#[test]
fn the_field_tick_advances_the_clock_and_counts_the_gates() {
    let mut field: Vec<IntroParticle> = (0..PARTICLE_TICK_COUNT + 8)
        .map(|i| IntroParticle {
            delay: (i % 200) as i16,
            tint: if i % 100 == 0 {
                0xFF00_0000
            } else {
                0x0080_8080
            },
            ..particle()
        })
        .collect();
    let mut elapsed = 2i16;
    let tick = tick_particle_field(&mut field, &PARTICLE_TICK_A, &mut elapsed, 3);
    assert!(tick.not_first_frame && tick.late_wash);
    assert_eq!(elapsed, 5);
    assert!(tick.masked > 0 && tick.moved > 0);
    // The tail past 0x488 is untouched.
    assert_eq!(field[PARTICLE_TICK_COUNT].rot, particle().rot);
}

fn quad_table() -> Vec<IntroQuadDesc> {
    vec![
        IntroQuadDesc {
            size_q12: 0x1000,
            w: 8,
            h: 8,
            ..Default::default()
        };
        8
    ]
}

#[test]
fn the_row_pass_slices_the_whole_screen_width_in_two() {
    assert_eq!(
        i32::from(CURTAIN_LEFT_W) + i32::from(CURTAIN_RIGHT_W),
        CURTAIN_COLS
    );
    let mut table = quad_table();
    let mut elapsed = 0i16;
    let tick = tick_curtain(&mut table, &mut elapsed, 1);
    let rows: Vec<_> = tick
        .quads
        .iter()
        .filter(|q| q.desc_index == CURTAIN_ROW_DESC)
        .collect();
    assert_eq!(rows.len(), 2 * CURTAIN_ROWS as usize);
    assert_eq!(rows[0].request.x, 0);
    assert_eq!(rows[1].request.x, i16::from(CURTAIN_LEFT_W));
    assert_eq!(elapsed, 1);
    assert!(!tick.not_first_frame, "the style always clears the flag");
}

#[test]
fn at_clock_zero_the_row_warp_is_the_identity() {
    let mut table = quad_table();
    let mut elapsed = 0i16;
    let tick = tick_curtain(&mut table, &mut elapsed, 1);
    for row in 0..CURTAIN_ROWS {
        assert_eq!(tick.quads[row as usize * 2].request.y, row as i16);
    }
}

#[test]
fn the_row_warp_pushes_away_from_the_centre_as_the_clock_runs() {
    let mut table = quad_table();
    let mut elapsed = 28i16;
    let tick = tick_curtain(&mut table, &mut elapsed, 1);
    // (0 - 120) * (28 + 28) / 28 + 120 == -240 + 120 == -120.
    assert_eq!(tick.quads[0].request.y, -120);
    // The centre row never moves.
    let centre = CURTAIN_ROW_CENTRE as usize * 2;
    assert_eq!(tick.quads[centre].request.y, CURTAIN_ROW_CENTRE as i16);
}

#[test]
fn every_column_is_visible_at_clock_zero_and_most_are_culled_later() {
    let mut table = quad_table();
    let mut elapsed = 0i16;
    let tick = tick_curtain(&mut table, &mut elapsed, 1);
    assert_eq!(tick.culled_columns, 0);
    let cols: Vec<_> = tick
        .quads
        .iter()
        .filter(|q| q.desc_index == CURTAIN_COL_DESC)
        .collect();
    assert_eq!(cols.len(), CURTAIN_COLS as usize);
    assert_eq!(cols[0].request.x, -CURTAIN_COL_CENTRE as i16 + 0x1E0);
    assert_eq!(cols[0].request.scale_y, 0x1000, "no stretch at clock zero");

    let mut elapsed = 100i16;
    let later = tick_curtain(&mut table, &mut elapsed, 1);
    assert!(later.culled_columns > 0);
}

#[test]
fn the_column_pass_swaps_page_and_biases_u_past_the_split() {
    let mut table = quad_table();
    let mut elapsed = 0i16;
    tick_curtain(&mut table, &mut elapsed, 1);
    // The last column processed leaves its patch behind.
    assert_eq!(table[CURTAIN_COL_DESC].tpage, CURTAIN_COL_TPAGE_RIGHT);
    assert_eq!(
        table[CURTAIN_COL_DESC].u0,
        ((CURTAIN_COLS - 1 + 0x40) as u8)
    );
}
