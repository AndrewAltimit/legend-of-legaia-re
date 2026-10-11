use super::*;

#[test]
fn the_base_ramp_constants_decode_to_their_documented_channels() {
    assert_eq!(ambient_base_rgb(AMBIENT_BASE_FLOOR), [0x20; 3]);
    assert_eq!(ambient_base_rgb(AMBIENT_BASE_CEIL), [0x80; 3]);
    assert_eq!(ambient_base_rgb(AMBIENT_BASE_CAST_EXIT), [0x21; 3]);
}

#[test]
fn a_cast_dims_eight_a_vsync_and_the_fight_recovers_two_a_vsync() {
    let mut b = AMBIENT_BASE_CEIL;
    let mut frames = 0;
    while b != AMBIENT_BASE_FLOOR {
        b = ambient_base_step(b, true, 1);
        frames += 1;
        assert!(frames < 100);
    }
    // (0x80 - 0x20) / 8.
    assert_eq!(frames, 12);
    assert_eq!(
        ambient_base_rgb(ambient_base_step(AMBIENT_BASE_CEIL, true, 1)),
        [0x78; 3]
    );
    // The step is `dt`-scaled: a two-vsync frame drops sixteen.
    assert_eq!(
        ambient_base_rgb(ambient_base_step(AMBIENT_BASE_CEIL, true, 2)),
        [0x70; 3]
    );
    let mut frames = 0;
    while b != AMBIENT_BASE_CEIL {
        b = ambient_base_step(b, false, 1);
        frames += 1;
        assert!(frames < 100);
    }
    assert_eq!(frames, 48);
    // The cast exit's odd seed overshoots the ceiling and is clamped.
    let mut b = AMBIENT_BASE_CAST_EXIT;
    for _ in 0..47 {
        b = ambient_base_step(b, false, 1);
    }
    assert_eq!(ambient_base_rgb(b), [0x7F; 3]);
    assert_eq!(ambient_base_step(b, false, 1), AMBIENT_BASE_CEIL);
}

#[test]
fn the_summon_captures_ambients_are_points_on_the_ramp() {
    // `0x606060` / `0x686868` / `0x707070` = base `0x20` / `0x28` /
    // `0x30` plus `0x404040`.
    let mut seen = std::collections::BTreeSet::new();
    let mut b = AMBIENT_BASE_CEIL;
    for _ in 0..20 {
        b = ambient_base_step(b, true, 2);
        seen.insert(battle_ambient_colour(ambient_base_rgb(b))[0]);
    }
    assert!(seen.contains(&0x60) && seen.contains(&0x70));
    let mut b = AMBIENT_BASE_FLOOR;
    b = ambient_base_step(b, false, 2);
    b = ambient_base_step(b, false, 2);
    assert_eq!(battle_ambient_colour(ambient_base_rgb(b)), [0x68; 3]);
}

#[test]
fn the_store_skips_only_on_the_floor_under_the_two_latches() {
    assert!(!ambient_store_skipped([0x20; 3], 1, 0));
    assert!(ambient_store_skipped([0x20; 3], 2, 0));
    assert!(ambient_store_skipped([0x20; 3], 0, 1));
    assert!(!ambient_store_skipped([0x21; 3], 2, 1));
}

#[test]
fn origin_is_x_centred_and_z_biased_one_cell() {
    // The 28x28 grid the live battle capture reports.
    assert_eq!(grid_origin(28, 28), (-7168, -7680));
    // X is symmetric about zero; Z is a whole cell further out.
    let (x, z) = grid_origin(28, 28);
    assert_eq!(z, x - CELL_PITCH);
}

#[test]
fn odd_cell_counts_round_toward_zero_like_an_sra() {
    // `sra` of a positive halfword truncates, so 29 and 28 share an origin.
    assert_eq!(grid_origin(29, 29), grid_origin(28, 28));
}

#[test]
fn probe_points_sit_a_half_step_inside_the_cell() {
    let (x0, z0) = grid_origin(4, 4);
    assert_eq!(probe_point(4, 4, 0, 0), (x0 + 0x100, z0 + 0x100));
    assert_eq!(probe_point(4, 4, 3, 2), (x0 + 0x700, z0 + 0x500));
}

#[test]
fn depth_classes_split_at_the_two_documented_bounds() {
    assert_eq!(classify_cell(-NEAR_BIAS), CellVis::Behind);
    assert_eq!(classify_cell(-NEAR_BIAS - 1), CellVis::Behind);
    assert_eq!(classify_cell(-NEAR_BIAS + 1), CellVis::Visible);
    // The far test is on the biased depth, so the world-space cutoff is
    // FAR_LIMIT - NEAR_BIAS = 0x6500.
    assert_eq!(classify_cell(0x6500), CellVis::Visible);
    assert_eq!(classify_cell(0x6501), CellVis::Far);
    assert_eq!(CellVis::Behind.as_byte(), -1);
    assert_eq!(CellVis::Far.as_byte(), 0);
    assert_eq!(CellVis::Visible.as_byte(), 1);
}

#[test]
fn implied_y_is_zero_for_the_positive_half_and_minus_one_for_the_other() {
    assert_eq!(implied_y(0), 0);
    assert_eq!(implied_y(0x7FFF), 0);
    assert_eq!(implied_y(-1), -1);
    assert_eq!(implied_y(-0x7000), -1);
}

#[test]
fn lattice_is_three_by_three_at_the_sub_step() {
    let l = cell_lattice(4, 4, 1, 1);
    let (x0, z0) = grid_origin(4, 4);
    assert_eq!(l[0][0], (x0 + CELL_PITCH, z0 + CELL_PITCH));
    assert_eq!(l[0][2], (x0 + CELL_PITCH + 0x200, z0 + CELL_PITCH));
    assert_eq!(l[2][2], (x0 + CELL_PITCH + 0x200, z0 + CELL_PITCH + 0x200));
    // Middle samples are the sub-step, i.e. the shared edge of the 2x2.
    assert_eq!(l[1][1], (x0 + CELL_PITCH + 0x100, z0 + CELL_PITCH + 0x100));
}

#[test]
fn screen_reject_is_an_aabb_overlap_with_the_viewport() {
    let on = [(10, 10), (60, 10), (10, 60), (60, 60)];
    assert!(quad_on_screen(on));
    // Entirely off the left edge.
    let left = [(-40, 10), (-1, 10), (-40, 60), (-1, 60)];
    assert!(!quad_on_screen(left));
    // Entirely below the bottom edge.
    let below = [(10, 240), (60, 240), (10, 300), (60, 300)];
    assert!(!quad_on_screen(below));
    // Entirely off the right edge.
    let right = [(320, 10), (400, 10), (320, 60), (400, 60)];
    assert!(!quad_on_screen(right));
    // Straddling: one corner inside is enough.
    let straddle = [(-40, -40), (10, -40), (-40, 10), (10, 10)];
    assert!(quad_on_screen(straddle));
}

#[test]
fn sub_tiles_tile_the_window_without_gaps_or_overlap() {
    let mut covered = std::collections::HashSet::new();
    for i in 0..SUB_QUADS_PER_CELL {
        let uv = sub_tile_uv(i);
        // Each tile is 0x20 wide and tall, expressed as inclusive bounds.
        assert_eq!(uv[1].0 - uv[0].0, 0x1F);
        assert_eq!(uv[2].1 - uv[0].1, 0x1F);
        for u in uv[0].0..=uv[1].0 {
            for v in uv[0].1..=uv[2].1 {
                assert!(covered.insert((u, v)), "texel ({u},{v}) claimed twice");
            }
        }
    }
    assert_eq!(covered.len(), 64 * 64, "the 64x64 window is fully covered");
    assert!(covered.iter().all(|&(u, v)| u >= 0xC0 && v >= 0xC0));
}

#[test]
fn sub_tile_order_follows_the_sub_quad_scan_order() {
    assert_eq!(sub_tile_index(0, 0), 0);
    assert_eq!(sub_tile_index(0, 1), 1);
    assert_eq!(sub_tile_index(1, 0), 2);
    assert_eq!(sub_tile_index(1, 1), 3);
    // u rises with the column, v with the row.
    assert!(sub_tile_uv(1)[0].0 > sub_tile_uv(0)[0].0);
    assert_eq!(sub_tile_uv(1)[0].1, sub_tile_uv(0)[0].1);
    assert!(sub_tile_uv(2)[0].1 > sub_tile_uv(0)[0].1);
    assert_eq!(sub_tile_uv(2)[0].0, sub_tile_uv(0)[0].0);
}

#[test]
fn sub_quads_partition_the_lattice() {
    let mut seen = std::collections::HashSet::new();
    for r in 0..2 {
        for c in 0..2 {
            for idx in sub_quad_lattice(r, c) {
                assert!(idx.0 < 3 && idx.1 < 3);
                seen.insert(idx);
            }
        }
    }
    assert_eq!(seen.len(), 9, "all nine lattice points are used");
}

/// The `DPCS` kernel, as this crate's test-local mirror of
/// `engine_render::psx_light::depth_cue` (extrapolate, then clamp -
/// the GTE's colour-FIFO saturation).
fn dpcs(c: f32, fc: f32, ir0: f32) -> f32 {
    (c + (fc - c) * ir0).clamp(0.0, 255.0)
}

#[test]
fn ir0_is_sz_over_four_on_the_gte_scale() {
    assert_eq!(grid_ir0_raw(0), 0);
    assert_eq!(grid_ir0_raw(0x1000), 0x400);
    // Full blend at SZ = 0x4000...
    assert_eq!(grid_ir0_raw(GRID_IR0_FULL_DEPTH), GRID_IR0_ONE);
    assert!((grid_ir0(GRID_IR0_FULL_DEPTH) - 1.0).abs() < 1e-6);
    // ...and deliberately unsaturated past it, like the mtc2 load.
    assert!(grid_ir0_raw(0x6500) > GRID_IR0_ONE);
    assert!((grid_ir0(0x6500) - (0x6500 as f32 / 0x4000 as f32)).abs() < 1e-3);
}

#[test]
fn cue_ramp_constants_reproduce_the_per_vertex_law() {
    // The linear ramp a host stages - ir0(z) = clamp(z / far_z, 0, 1)
    // * max_ir0 - must equal SZ >> 2 (as a fraction of 0x1000) across
    // the whole drawable depth range.
    let far_z = grid_cue_far_z();
    let max = grid_cue_max_ir0();
    assert!(max > 1.0, "the grid's cue extrapolates past the far colour");
    for z in (0..=GRID_CUE_FAR_Z).step_by(0x100) {
        let ramp = (z as f32 / far_z).clamp(0.0, 1.0) * max;
        let law = grid_ir0(z & !0x3); // the srl truncates the low bits
        assert!(
            (ramp - law).abs() < 1.5e-3,
            "z={z:#x}: ramp {ramp} != law {law}"
        );
    }
}

#[test]
fn far_colour_arms_reproduce_the_captured_values() {
    // Settled captures: indoor (Queen Bee / Gimard, town01 stages) and
    // outdoor (vs Gobu Gobu, stage id 0x55 = map01).
    assert_eq!(
        grid_far_colour(GRID_FAR_BASE_NEUTRAL, false),
        GRID_FAR_INDOOR
    );
    assert_eq!(
        grid_far_colour(GRID_FAR_BASE_NEUTRAL, true),
        GRID_FAR_OUTDOOR
    );
    // The battle-intro fade sample: the same indoor arm over the
    // ramping base (captured (6,6,6) at base 0x0C0C0C, frame 1).
    assert_eq!(grid_far_colour([0x0C; 3], false), [0x06; 3]);
}

#[test]
fn dpcs_at_the_captured_far_colours_pins_the_drawn_packet_colour() {
    // The grid quads' pre-cue colour is the settled battle ambient
    // (`RGBC` <- `0x1F800398` <- `0x8007B7B0`), not the neutral `0x80`.
    let near = GRID_RGBC_SETTLED[0] as f32;
    // Indoor: full blend lands exactly on the far colour...
    assert_eq!(dpcs(near, 0x40 as f32, 1.0), 0x40 as f32);
    // ...and the far cull edge extrapolates past it to black
    // (SZ = 0x6500, IR0 ~ 1.58).
    assert_eq!(dpcs(near, 0x40 as f32, grid_ir0(0x6500)), 0.0);
    // Outdoor: brightens toward 0xFE and saturates just past full
    // blend rather than overshooting.
    assert_eq!(dpcs(near, 0xFE as f32, 1.0), 0xFE as f32);
    assert_eq!(dpcs(near, 0xFE as f32, grid_ir0(0x6500)), 255.0);
    // ir0 = 0 is the identity - the near edge draws the ambient.
    assert_eq!(dpcs(near, 0x40 as f32, 0.0), near);
}

#[test]
fn the_ambient_is_the_base_lifted_by_0x40() {
    assert_eq!(
        battle_ambient_colour(GRID_FAR_BASE_NEUTRAL),
        GRID_RGBC_SETTLED
    );
    // The intro ramp sample base (0x0C0C0C) and a saturating channel.
    assert_eq!(battle_ambient_colour([0x0C; 3]), [0x4C; 3]);
    assert_eq!(battle_ambient_colour([0xF0, 0, 0x80]), [0xFF, 0x40, 0xC0]);
}

#[test]
fn outdoor_table_parses_and_classifies() {
    let mut scus = vec![0u8; OUTDOOR_CUE_TABLE_SCUS_OFFSET + 16];
    scus[OUTDOOR_CUE_TABLE_SCUS_OFFSET..OUTDOOR_CUE_TABLE_SCUS_OFFSET + 6]
        .copy_from_slice(&[0x55, 0x00, 0x9E, 0x00, 0x00, 0x00]);
    let t = OutdoorCueTable::from_scus(&scus).expect("table");
    assert_eq!(t.ids(), &[0x55, 0x9E]);
    assert!(t.contains_runtime_id(0x55));
    assert!(!t.contains_runtime_id(0x15));
    // PROT keying: runtime id + 3 (0x55 -> PROT 88 = map01's stage).
    assert!(t.contains_prot_index(88));
    assert!(!t.contains_prot_index(24)); // 0x15 + 3, the Queen Bee stage
    assert_eq!(t.far_colour_for_prot_index(88), GRID_FAR_OUTDOOR);
    assert_eq!(t.far_colour_for_prot_index(24), GRID_FAR_INDOOR);
    // Unterminated / short buffers refuse rather than fabricate.
    assert!(OutdoorCueTable::from_scus(&[0u8; 16]).is_none());
    let junk = vec![0x11u8; OUTDOOR_CUE_TABLE_SCUS_OFFSET + 4096];
    assert!(OutdoorCueTable::from_scus(&junk).is_none());
}

#[test]
fn classify_walks_row_major_and_counts_the_emitted_quads() {
    // A projector that only keeps the second row in range.
    let (_, z_min) = grid_origin(4, 3);
    let grid = GroundGrid::classify(4, 3, |_x, z| {
        if z == z_min + SUB_STEP + CELL_PITCH {
            0x1000
        } else {
            -0x8000
        }
    });
    assert_eq!(grid.vis.len(), 12);
    assert_eq!(grid.visible_cells(), 4);
    assert_eq!(grid.max_quads(), 16);
    for col in 0..4 {
        assert_eq!(grid.cell(col, 1), Some(1));
        assert_eq!(grid.cell(col, 0), Some(-1));
    }
    assert_eq!(grid.cell(4, 0), None);
}
