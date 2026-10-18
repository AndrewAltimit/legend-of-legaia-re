use super::*;

fn flat(_: f32, _: f32) -> f32 {
    0.0
}

fn raised() -> FogVolume {
    let mut f = FogVolume::new();
    // Full strength immediately, so the tests need no fade-in ticks.
    f.set_style(Some(POOL_STYLE));
    f.strength = 1.0;
    f
}

/// A mover walking +X across the grid for `ticks` ticks at `speed`
/// units per tick, starting at `x0`.
fn walk(f: &mut FogVolume, x0: f32, speed: f32, ticks: u32) -> f32 {
    let mut x = x0;
    for _ in 0..ticks {
        f.step([0.0, 0.0], &[FogMover { key: 1, x, z: 0.0 }], flat);
        x += speed;
    }
    x
}

#[test]
fn same_input_same_bank_bit_for_bit() {
    let mut a = raised();
    let mut b = raised();
    walk(&mut a, -600.0, 4.0, 200);
    walk(&mut b, -600.0, 4.0, 200);
    assert_eq!(a.density, b.density);
    assert_eq!(a.velocity, b.velocity);
    let fa = a.frame(None, None).unwrap();
    let fb = b.frame(None, None).unwrap();
    assert_eq!(fa.density, fb.density);
    assert_eq!(fa.header(), fb.header());
}

#[test]
fn a_walker_leaves_a_wake_and_untouched_bank_ahead() {
    let mut f = raised();
    let end = walk(&mut f, -600.0, 4.0, 150);
    // Behind the walker (its path), well cleared; ahead, untouched.
    let behind = f.density_at(end - 120.0, 0.0);
    let ahead = f.density_at(end + 400.0, 0.0);
    let beside = f.density_at(end - 120.0, 400.0);
    assert!(behind < 0.6, "wake behind the walker: {behind}");
    assert!(ahead > 0.97, "bank ahead untouched: {ahead}");
    assert!(
        beside > 0.95,
        "bank well beside the path untouched: {beside}"
    );
}

#[test]
fn the_wake_refills_once_the_walker_is_gone() {
    let mut f = raised();
    let end = walk(&mut f, -600.0, 4.0, 150);
    let probe = end - 120.0;
    let carved = f.density_at(probe, 0.0);
    // Walk away out of the grid's reach, then wait.
    for _ in 0..600 {
        f.step([0.0, 0.0], &[], flat);
    }
    let refilled = f.density_at(probe, 0.0);
    assert!(refilled > carved + 0.3, "{carved} -> {refilled}");
    assert!(refilled > 0.9, "refilled {refilled}");
}

#[test]
fn a_stride_carves_harder_than_standing_still() {
    let mut idle = raised();
    let mut moving = raised();
    for _ in 0..30 {
        idle.step(
            [0.0, 0.0],
            &[FogMover {
                key: 1,
                x: 0.0,
                z: 0.0,
            }],
            flat,
        );
    }
    walk(&mut moving, -60.0, 4.0, 30);
    // Compare the darkest cell each one leaves.
    let min = |f: &FogVolume| f.density.iter().cloned().fold(1.0f32, f32::min);
    assert!(
        min(&moving) < min(&idle),
        "{} vs {}",
        min(&moving),
        min(&idle)
    );
    assert!(min(&idle) < 1.0, "standing still opens a pocket");
}

#[test]
fn a_teleport_injects_no_velocity() {
    let mut f = raised();
    f.step(
        [0.0, 0.0],
        &[FogMover {
            key: 7,
            x: -500.0,
            z: 0.0,
        }],
        flat,
    );
    f.step(
        [0.0, 0.0],
        &[FogMover {
            key: 7,
            x: 500.0,
            z: 0.0,
        }],
        flat,
    );
    let vmax = f
        .velocity
        .iter()
        .map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt())
        .fold(0.0f32, f32::max);
    assert!(vmax < 1.0e-6, "teleport stirred the bank: {vmax}");
}

#[test]
fn recentring_keeps_the_wake_where_it_was_carved() {
    let mut f = raised();
    let end = walk(&mut f, -200.0, 4.0, 60);
    let probe = end - 60.0;
    let before = f.density_at(probe, 0.0);
    // Shift the focus by several cells without stepping the sim.
    f.recentre([400.0, 120.0], flat);
    let after = f.density_at(probe, 0.0);
    assert!((before - after).abs() < 1.0e-6, "{before} vs {after}");
}

#[test]
fn the_frame_is_none_while_no_scene_raises_a_bank() {
    let mut f = FogVolume::new();
    f.set_style(None);
    f.step([0.0, 0.0], &[], flat);
    assert!(f.frame(None, None).is_none());
    assert!(scene_style("town01", false).is_none());
    assert!(scene_style("town0b", false).is_some());
    assert_eq!(scene_style("anything", true), Some(POOL_STYLE));
}

#[test]
fn strength_eases_in_and_out() {
    let mut f = FogVolume::new();
    f.set_style(Some(POOL_STYLE));
    assert!(f.strength > 0.0 && f.strength < 1.0);
    for _ in 0..200 {
        f.set_style(Some(POOL_STYLE));
    }
    assert_eq!(f.strength, 1.0);
    f.set_style(None);
    assert!(
        f.strength < 1.0 && f.visible(),
        "fades out on the last style"
    );
    for _ in 0..200 {
        f.set_style(None);
    }
    assert!(!f.visible());
}

#[test]
fn a_dark_scene_dims_the_bank_and_a_bright_one_does_not() {
    let f = raised();
    let mut f = f;
    f.recentre([0.0, 0.0], flat);
    let bright = f.frame(None, Some(0.9)).unwrap().color;
    let dark = f.frame(None, Some(0.05)).unwrap().color;
    assert_eq!(bright, POOL_STYLE.color);
    assert!(luma(dark) <= FOG_LUMA_FLOOR + 0.05 * FOG_LUMA_OVER_SCENE + 1.0e-5);
    assert!(luma(dark) < luma(bright));
    // Hue kept: the channels scale together.
    let r = dark[0] / POOL_STYLE.color[0];
    assert!((dark[2] / POOL_STYLE.color[2] - r).abs() < 1.0e-5);
}

#[test]
fn scene_luminance_reads_lit_texels() {
    let mut vram = legaia_tim::Vram::new();
    // A 15-bit page at (0, 0): mid-grey 16/31 everywhere it is sampled.
    let grey: u16 = 16 | (16 << 5) | (16 << 10);
    let bytes: Vec<u8> = std::iter::repeat_n(grey.to_le_bytes(), 64 * 4)
        .flatten()
        .collect();
    vram.write_block(0, 0, 64, 4, &bytes);
    let mesh = legaia_tmd::mesh::VramMesh {
        positions: vec![[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 100.0]],
        uvs: vec![[0, 0], [2, 0], [0, 2]],
        cba_tsb: vec![[0, 2 << 7]; 3],
        indices: vec![0, 1, 2],
        normals: vec![[0.0; 3]; 3],
        colors: vec![[0x40; 3]; 3],
    };
    let l = scene_luminance(&vram, [&mesh]).unwrap();
    // grey 16/31 at half modulation.
    assert!((l - 16.0 / 31.0 * 0.5).abs() < 1.0e-3, "{l}");
}

#[test]
fn a_floor_step_clears_the_bank_off_the_cliff() {
    let mut f = raised();
    // A 200-unit cliff along x = 0: upper tier west of it.
    f.recentre([0.0, 0.0], |x, _| if x < 0.0 { -200.0 } else { 0.0 });
    let fr = f.frame(None, None).unwrap();
    let pos = fr.mesh_positions();
    for v in pos.chunks(4) {
        let near_cliff = v[0] > -2.0 * fr.mesh_cell && v[0] < 2.0 * fr.mesh_cell;
        if (v[0] + fr.mesh_cell / 2.0).abs() < fr.mesh_cell || v[0].abs() < 1.0 {
            assert_eq!(v[3], 0.0, "vertex at x {} stands on the step", v[0]);
        } else if !near_cliff {
            assert_eq!(v[3], 1.0, "vertex at x {} is open floor", v[0]);
        }
    }
}

#[test]
fn mesh_follows_the_floor_heights() {
    let mut f = raised();
    f.recentre([1000.0, 2000.0], |x, z| -(x + z) * 0.01);
    let fr = f.frame(None, None).unwrap();
    let pos = fr.mesh_positions();
    assert_eq!(pos.len(), (MESH_DIM + 1) * (MESH_DIM + 1) * 4);
    for v in pos.chunks(4) {
        assert!((v[1] - (-(v[0] + v[2]) * 0.01)).abs() < 1.0e-3);
        // A gentle slope (under 3 units per quad) is floor throughout.
        assert_eq!(v[3], 1.0);
    }
    assert_eq!(mesh_indices().len(), MESH_DIM * MESH_DIM * 6);
}

fn region(enabled: bool, x0: u8, z0: u8, x1: u8, z1: u8) -> FogRegion {
    FogRegion {
        enabled,
        x0,
        z0,
        x1,
        z1,
        angle_base: 0,
        angle_spread: 0,
        speed: 0,
        byte_8: 0,
        flag_index: 0,
    }
}

/// A lattice with a street (area 1, large), a room (area 2, small) and a
/// second street strip (area 3, small) reached on foot.
fn town() -> InteriorTracker {
    let mut labels = vec![0u16; AREA_STRIDE * AREA_STRIDE];
    let mut fill = |x0: usize, z0: usize, x1: usize, z1: usize, id: u16| {
        for z in z0..z1 {
            for x in x0..x1 {
                labels[z * AREA_STRIDE + x] = id;
            }
        }
    };
    fill(0, 0, 60, 40, 1); // street, 2400 sub-cells
    fill(200, 200, 210, 210, 2); // room, 100
    fill(61, 0, 70, 10, 3); // strip past a closed stair column, 90
    let mut t = InteriorTracker::default();
    t.seed(labels, vec![2400, 100, 90]);
    t
}

fn at(sx: i32, sz: i32) -> (i32, i32) {
    (sx * 64 + 32, sz * 64 + 32)
}

#[test]
fn a_door_warp_into_a_room_is_indoors_and_back_out_is_not() {
    let mut t = town();
    let (x, z) = at(10, 10);
    assert!(!t.observe(x, z));
    assert!(!t.indoors);
    // Door: one tick, far away, into the small area.
    let (x, z) = at(205, 205);
    assert!(t.observe(x, z), "the warp in reports the cut");
    assert!(t.indoors);
    // Walking about the room stays indoors and reports nothing new.
    let (x, z) = at(206, 205);
    assert!(!t.observe(x, z));
    assert!(t.indoors);
    // Door back out onto the street.
    let (x, z) = at(10, 12);
    assert!(!t.observe(x, z));
    assert!(!t.indoors);
    // And in again: the room is remembered.
    let (x, z) = at(205, 205);
    assert!(t.observe(x, z));
    assert!(t.indoors);
}

#[test]
fn walking_into_a_small_area_keeps_the_street_class() {
    let mut t = town();
    let (x, z) = at(58, 5);
    t.observe(x, z);
    // Two sub-cells east, across the closed column, on foot.
    for sx in [59, 60, 61, 62] {
        let (x, z) = at(sx, 5);
        assert!(!t.observe(x, z));
    }
    assert!(!t.indoors, "a stair-split strip is still the street");
    // Even a later warp into it keeps it open ground.
    let (x, z) = at(10, 30);
    t.observe(x, z);
    let (x, z) = at(65, 5);
    assert!(!t.observe(x, z));
    assert!(!t.indoors);
}

#[test]
fn a_warp_into_a_large_area_is_open_ground() {
    let mut t = town();
    // Enter the scene in the room (a card load): that is where the
    // player came in, so it counts as open ground.
    let (x, z) = at(205, 205);
    t.observe(x, z);
    assert!(!t.indoors);
    let (x, z) = at(10, 10);
    assert!(!t.observe(x, z));
    assert!(!t.indoors);
}

#[test]
fn region_weight_is_the_spawner_rule() {
    // No table: the whole scene.
    assert_eq!(region_weight(&[], 90, 90), 1.0);
    // A town box: open bounds, nothing outside it.
    let town = [region(true, 0, 0, 56, 48)];
    assert_eq!(region_weight(&town, 20, 20), 1.0);
    assert_eq!(region_weight(&town, 0, 20), 0.0, "open lower bound");
    assert_eq!(region_weight(&town, 56, 20), 0.0, "open upper bound");
    assert_eq!(region_weight(&town, 97, 54), 0.0, "a house room beside it");
    // A disabled box ahead of an area-wide one is a hole; the same box
    // enabled is fog like the rest.
    let holed = [region(false, 24, 0, 45, 22), region(true, 0, 0, 126, 126)];
    assert_eq!(region_weight(&holed, 30, 10), 0.0);
    assert_eq!(region_weight(&holed, 60, 60), 1.0);
    let filled = [region(true, 24, 0, 45, 22), region(true, 0, 0, 126, 126)];
    assert_eq!(region_weight(&filled, 30, 10), 1.0);
    // A disabled first hit ends the search even under an enabled box.
    let off = [region(false, 0, 0, 126, 126)];
    assert_eq!(region_weight(&off, 60, 60), 0.0);
}

#[test]
fn regions_mask_the_sheet_mesh_and_a_change_resamples_it() {
    let mut f = raised();
    let focus = [64.0 * 128.0, 64.0 * 128.0];
    f.recentre(focus, flat);
    let gen0 = f.ground_gen;
    let all = |f: &FogVolume| f.ground_weight.iter().all(|w| *w == 1.0);
    assert!(all(&f), "no table: every floor vertex carries bank");

    // A box over the west half of the mesh only.
    f.set_regions(&[region(true, 0, 0, 64, 127)]);
    assert!(f.recentre(focus, flat), "a new table re-samples in place");
    assert_ne!(f.ground_gen, gen0);
    let n = MESH_DIM + 1;
    for z in 0..n {
        for x in 0..n {
            let tx = f.mesh_origin[0] + x as i32;
            let tz = f.mesh_origin[1] + z as i32;
            let want = if 0 < tx && tx < 64 && 0 < tz && tz < 127 {
                1.0
            } else {
                0.0
            };
            assert_eq!(f.ground_weight[z * n + x], want, "tile ({tx}, {tz})");
        }
    }
    // The same table again does not re-sample.
    let gen1 = f.ground_gen;
    f.set_regions(&[region(true, 0, 0, 64, 127)]);
    assert!(!f.recentre(focus, flat));
    assert_eq!(f.ground_gen, gen1);

    // Battle space ignores the field table.
    f.set_space(FogSpace::Battle);
    f.recentre([0.0, 0.0], flat);
    assert!(all(&f));
}

#[test]
fn a_battle_bank_is_thin_and_low_and_eases_in() {
    // The battle profile: at most a third of the field's opacity and gain,
    // no deeper than the field style's height.
    let field = FogSpace::Field;
    let battle = FogSpace::Battle;
    assert!(battle.density_scale() <= field.density_scale() / 3.0 + 1.0e-6);
    assert!(battle.shader_constants()[2] <= field.shader_constants()[2]);
    assert!(battle.height_scale() <= 1.0);

    // Field -> battle: the bank restarts from nothing under the new profile
    // instead of carrying the field strength over (no pop).
    let mut f = raised();
    f.step([0.0, 0.0], &[], flat);
    let field_opacity = f.frame(None, None).unwrap().opacity;
    f.set_space(FogSpace::Battle);
    assert_eq!(f.strength, 0.0);
    f.set_style(Some(POOL_STYLE));
    f.step([0.0, 0.0], &[], flat);
    let first = f.frame(None, None).unwrap().opacity;
    assert!(first > 0.0 && first < 0.05 * field_opacity, "{first}");
    for _ in 0..200 {
        f.set_style(Some(POOL_STYLE));
        f.step([0.0, 0.0], &[], flat);
    }
    let settled = f.frame(None, None).unwrap().opacity;
    assert!((settled - field_opacity * BATTLE_FOG.density).abs() < 1.0e-4);
}
