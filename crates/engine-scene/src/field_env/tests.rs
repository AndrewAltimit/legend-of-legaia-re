use super::*;

fn tilted_draw(rot_x: u16, rot_y: u16, rot_z: u16) -> EnvDraw {
    EnvDraw {
        env_slot: 0,
        res_tmd: 0,
        world_x: 100,
        world_y: -50,
        world_z: 7,
        rot_y,
        rot_x,
        rot_z,
        anim_id: 0,
        anchor: (0, 0),
        floor: FloorAnchor {
            corners: None,
            nibble: None,
        },
        cell: (0, 0),
        cull_radius: 0,
        view_skip: 0,
    }
}

/// `place_point` is `Rx * Ry * Rz` (the right-handed per-axis matrices
/// the native window's `placement_rotation` builds with glam) applied to
/// the vertex, then the translation - checked against the explicit
/// matrix product, with all three axes non-zero so a reversed order or
/// a negated angle cannot pass.
#[test]
fn place_point_composes_rx_ry_rz_then_translates() {
    type M = [[f32; 3]; 3];
    let mul = |a: M, b: M| -> M {
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
    };
    let ang = |v: u16| f32::from(v) * std::f32::consts::TAU / 4096.0;
    let (rx, ry, rz) = (0x0123u16, 0x0456u16, 0x0789u16);
    let (sx, cx) = ang(rx).sin_cos();
    let (sy, cy) = ang(ry).sin_cos();
    let (sz, cz) = ang(rz).sin_cos();
    let mx: M = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let my: M = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let mz: M = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let m = mul(mul(mx, my), mz);
    let v = [13.0f32, -29.0, 41.0];
    let lift = [0.5, -0.25, 1.0];
    let want: [f32; 3] = std::array::from_fn(|r| {
        (0..3).map(|k| m[r][k] * v[k]).sum::<f32>() + [100.0, -50.0, 7.0][r] + lift[r]
    });
    let got = tilted_draw(rx, ry, rz).place_point(v, lift);
    for i in 0..3 {
        assert!(
            (got[i] - want[i]).abs() < 1e-3,
            "axis {i}: {got:?} vs {want:?}"
        );
    }
    // Pure yaw reduces to the yaw-only bake the venues used to run.
    let (s, c) = ang(0x400).sin_cos();
    let y = tilted_draw(0, 0x400, 0).place_point(v, [0.0; 3]);
    assert!((y[0] - (v[0] * c + v[2] * s + 100.0)).abs() < 1e-3);
    assert!((y[2] - (-v[0] * s + v[2] * c + 7.0)).abs() < 1e-3);
}

/// `slti v0,v0,0xa0` at `0x8001B1C0`: a view depth of `0xA0` draws, one
/// below it does not, and an origin behind the eye never does.
#[test]
fn placed_near_reject_is_retails_case5_threshold() {
    assert!(!placed_origin_near_culled(160.0));
    assert!(!placed_origin_near_culled(2000.0));
    assert!(placed_origin_near_culled(159.5));
    assert!(placed_origin_near_culled(0.0));
    assert!(placed_origin_near_culled(-423.0));
}

#[test]
fn prop_pose_key_drops_what_cannot_change_the_pose() {
    let mut a = PropAnim::spawned(3, 4, false, 0);
    a.cursor = 16 + 8;
    // Ungated clip: the fraction never reaches the blender.
    assert_eq!(a.pose_key().cursor, 16);
    a.scaled_step = true;
    assert_eq!(a.pose_key().cursor, 24);
    assert!(
        !a.pose_key().hold_at_end,
        "clamp only matters on the last frame"
    );
    a.cursor = 3 * 16 + 15;
    a.flags |= ANIM_CLAMP;
    assert!(a.pose_key().hold_at_end);
    a.cursor = 0;
    assert!(a.pose_key().is_rest());
    let k = PropPoseKey {
        cursor: 63,
        hold_at_end: true,
    };
    assert_eq!(PropPoseKey::from_i32(k.to_i32()), k);
    assert!(k.to_i32() >= 0);
}

fn placement(pack_index: Option<u16>, nibble: Option<u8>, y_off: i16) -> Placement {
    Placement {
        obj_idx: 0,
        col: 2,
        row: 3,
        anchor_col: 2,
        anchor_row: 3,
        anchor_cell: 0,
        world_x: 2 * 0x80 + 0x40,
        world_z: 3 * 0x80 + 0x40,
        y_off,
        floor_nibble: nibble,
        floor_corner_nibbles: None,
        pack_index,
        flags: 0x4,
        rot_x: 0,
        rot_y: 0x400,
        rot_z: 0,
        collider_x: 0,
        collider_z: 0,
        cull_radius: 0,
    }
}

#[test]
fn draws_resolve_pack_index_and_floor_y() {
    let env_tmds = vec![10, 11, 12];
    let mut lut = [0i16; 16];
    lut[6] = 192;
    let placements = vec![placement(Some(2), Some(6), 8)];
    let (draws, drops) = resolve_env_draws(&env_tmds, &placements, Some(lut));
    assert!(drops.is_empty());
    assert_eq!(
        draws,
        vec![EnvDraw {
            env_slot: 2,
            res_tmd: 12,
            world_x: 2 * 0x80 + 0x40,
            world_y: -192 + 8,
            world_z: 3 * 0x80 + 0x40,
            rot_y: 0x400,
            rot_x: 0,
            rot_z: 0,
            anim_id: 0,
            anchor: (2, 3),
            // A placed object carries the single placement-tile nibble,
            // which is what the per-frame floor wave re-resolves it from.
            floor: FloorAnchor {
                corners: None,
                nibble: Some(6),
            },
            cell: (2, 3),
            cull_radius: 0,
            view_skip: 0,
        }]
    );
}

/// A bound draw whose record follows the floor takes the sampled floor Y,
/// dropping the record's `y_off` lift; an unbound draw and a bound draw
/// whose record keeps its seat stay where the sweep put them.
#[test]
fn floor_following_records_drop_their_lift() {
    let env_tmds = vec![10, 11, 12];
    let mut lut = [0i16; 16];
    lut[15] = 480;
    let mut follows = placement(Some(1), Some(15), 2080);
    follows.anchor_col = 7;
    let mut keeps = placement(Some(2), Some(15), -56);
    keeps.anchor_col = 8;
    let free = placement(Some(0), Some(15), 300);
    let mut binds = HashMap::new();
    binds.insert(
        (7u8, 3u8),
        ObjectBind {
            record: 0,
            anim_id: 0,
        },
    );
    binds.insert(
        (8u8, 3u8),
        ObjectBind {
            record: 4,
            anim_id: 0,
        },
    );
    let (mut draws, _) =
        resolve_placed_env_draws(&env_tmds, &[follows, keeps, free], Some(lut), Some(&binds));
    assert_eq!(
        draws.iter().map(|d| d.world_y).collect::<Vec<_>>(),
        [1600, -536, -180]
    );
    follow_floor_placed_draws(&mut draws, &binds, |r, _, _| (r == 0).then_some(-480));
    assert_eq!(
        draws.iter().map(|d| d.world_y).collect::<Vec<_>>(),
        [-480, -536, -180]
    );
}

/// The bind carries the `anim_id` onto the draw (so a multi-object prop gets
/// posed), but it is **not** a spawn gate: an unbound placement is the *other*
/// retail sweep's actor (`FUN_801D7B50`) and still draws, unposed. Only a
/// placement that is unbound **and** `CELL_BIND_OWNED` on its anchor tile -
/// which neither sweep would take - is dropped.
#[test]
fn binds_carry_the_anim_id_and_only_bind_owned_cells_drop() {
    let env_tmds = vec![10, 11, 12];
    let mut bound = placement(Some(1), None, 0);
    bound.anchor_col = 7;
    bound.anchor_row = 9;
    bound.anchor_cell = legaia_scene_formats::field_objects::CELL_BIND_OWNED;
    let unbound = placement(Some(2), None, 0); // anchor (2, 3): no bind, no 0x400
    let mut binds = HashMap::new();
    binds.insert(
        (7u8, 9u8),
        ObjectBind {
            record: 12,
            anim_id: 2,
        },
    );

    let (draws, drops) = resolve_placed_env_draws(&env_tmds, &[bound, unbound], None, Some(&binds));
    assert_eq!(
        draws.len(),
        2,
        "the unbound placement is the window sweep's"
    );
    assert_eq!((draws[0].env_slot, draws[0].anim_id), (1, 2));
    assert_eq!(
        (draws[1].env_slot, draws[1].anim_id),
        (2, 0),
        "the window sweep does no bind lookup, so its actor has no clip"
    );
    assert!(drops.is_empty());

    // Unbound *and* bind-owned: neither retail sweep takes it.
    let mut orphan = placement(Some(2), None, 0);
    orphan.anchor_cell = legaia_scene_formats::field_objects::CELL_BIND_OWNED;
    let (draws, drops) = resolve_placed_env_draws(&env_tmds, &[orphan], None, Some(&binds));
    assert!(draws.is_empty());
    assert!(matches!(
        drops.as_slice(),
        [EnvDrawDrop::Unbound { anchor: (2, 3), .. }]
    ));

    // Without binds (the terrain-cell sweeps) both draw, unposed.
    let (draws, drops) = resolve_placed_env_draws(&env_tmds, &[bound, unbound], None, None);
    assert_eq!(draws.len(), 2);
    assert!(draws.iter().all(|d| d.anim_id == 0));
    assert!(drops.is_empty());
}

/// The bind's anim id is the trailing byte of the partition-0 record's
/// `[u8 n][n*2 name][u8 anim]` header.
#[test]
fn partition0_header_yields_the_anim_id() {
    // Two records: #0 with a 3-"char" name and anim 2, #1 with no name
    // and anim 0.
    let dro = 0x40usize;
    let mut man = vec![0u8; dro];
    let mut offsets = Vec::new();
    for (name_len, anim) in [(3u8, 2u8), (0, 0)] {
        offsets.push((man.len() - dro) as u32);
        man.push(name_len);
        man.extend(std::iter::repeat_n(0x82u8, name_len as usize * 2));
        man.push(anim);
        man.extend_from_slice(&[0x21, 0x00]); // a token script body
    }
    let mf = ManFile {
        header: legaia_scene_formats::man_section::ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [2, 0, 0],
            u24_at_28: 0,
        },
        partitions: [offsets, Vec::new(), Vec::new()],
        data_region_offset: dro,
        sections: std::array::from_fn(|_| legaia_scene_formats::man_section::SectionRef {
            offset: man.len(),
            length: 0,
        }),
    };
    assert_eq!(partition0_anim_id(&mf, &man, 0), Some(2));
    assert_eq!(partition0_anim_id(&mf, &man, 1), Some(0));
    assert_eq!(partition0_anim_id(&mf, &man, 2), None);
}

#[test]
fn drops_classify_missing_and_out_of_range() {
    let env_tmds = vec![10];
    let placements = vec![placement(None, None, 0), placement(Some(5), None, 0)];
    let (draws, drops) = resolve_env_draws(&env_tmds, &placements, None);
    assert!(draws.is_empty());
    assert!(matches!(drops[0], EnvDrawDrop::NoPackIndex { .. }));
    assert!(matches!(
        drops[1],
        EnvDrawDrop::SlotOutOfRange { pack_index: 5, .. }
    ));
}

/// A terrain / decoration cell (corner block present) resolves world Y
/// through retail's per-cell emitter math (`FUN_801F69D8` /
/// `FUN_801F7088`): the flat mean of the four corner tiles' `-lut[nibble]`
/// heights, rounded toward zero, plus `y_off` - NOT the single-nibble
/// placement formula. A tier-edge ramp cell (`[0, 4, 0, 4]`) must land
/// mid-slope; snapping it to either tier is the Vidna stair-step defect.
#[test]
fn terrain_corner_block_resolves_to_the_corner_average() {
    let env_tmds = vec![10];
    // The retail LUT shape: `lut[n] = n * 32` (the balden ramp).
    let mut lut = [0i16; 16];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = (i as i16) * 32;
    }
    // balden (61, 2): corners [0, 4, 0, 4], y_off 64 -> mean of
    // (0, -128, 0, -128) = -256/4 = -64 (exact), +64 = 0.
    let mut ramp = placement(Some(0), Some(0), 64);
    ramp.floor_corner_nibbles = Some([0, 4, 0, 4]);
    let (draws, _) = resolve_env_draws(&env_tmds, &[ramp], Some(lut));
    assert_eq!(draws[0].world_y, 0, "mid-slope, not either tier");

    // balden (62, 2): corners [4, 8, 4, 8], y_off 64 -> mean of
    // (-128, -256, -128, -256) = -192, +64 = -128.
    let mut ramp2 = placement(Some(0), Some(4), 64);
    ramp2.floor_corner_nibbles = Some([4, 8, 4, 8]);
    let (draws, _) = resolve_env_draws(&env_tmds, &[ramp2], Some(lut));
    assert_eq!(draws[0].world_y, -128);

    // Round toward zero on an inexact negative sum, retail's
    // `if (sum < 0) sum += 3; sum >>= 2`: corners [1, 0, 0, 0] with
    // lut[1] = 34 -> sum -34 -> trunc(-8.5) = -8 (a plain `>> 2` floors
    // to -9).
    let mut lut2 = [0i16; 16];
    lut2[1] = 34;
    let mut odd = placement(Some(0), Some(1), 0);
    odd.floor_corner_nibbles = Some([1, 0, 0, 0]);
    let (draws, _) = resolve_env_draws(&env_tmds, &[odd], Some(lut2));
    assert_eq!(draws[0].world_y, -8, "negative sum rounds toward zero");

    // Without the corner block (a placed object) the single-nibble
    // placement formula still applies: -lut[nib] + y_off.
    let placed = placement(Some(0), Some(4), 64);
    let (draws, _) = resolve_env_draws(&env_tmds, &[placed], Some(lut));
    assert_eq!(draws[0].world_y, -128 + 64);
}

/// The **binder** half of `FUN_800204F8`, as a cross-context poke reaches
/// it: on a clip change it remembers the id and zeroes the frame cursor
/// (`sh zero,0x68(s0)`) and nothing else - `+0x62` stays exactly as the
/// script left it, because hold / clamp / reverse and the end latch are the
/// script's own `2B`/`2C` ops. A re-poke of the id already bound is skipped
/// whole (`lh v1,0x5c` / `lh v0,0x5e` / `beq v1,v0`), so it does not
/// restart a clip mid-play.
#[test]
fn a_cross_context_poke_zeroes_the_cursor_and_leaves_the_control_word() {
    let mut bank = PropAnimBank::default();
    bank.bind_actor_clip(PLAYER_ANCHOR_TARGET, 4, 15);
    // Let the script write the control word the way the innkeeper's does.
    let a = bank.actor_clip_mut(PLAYER_ANCHOR_TARGET).unwrap();
    a.flags = (a.flags & !ANIM_HOLD) | ANIM_CLAMP;
    a.cursor = 40;
    let marked = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().flags;

    // Same id again: no rebind, so the cursor keeps playing.
    bank.bind_actor_clip(PLAYER_ANCHOR_TARGET, 4, 15);
    let a = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap();
    assert_eq!(a.cursor, 40, "a re-poke of the bound id must not restart");
    assert_eq!(a.flags, marked);

    // A different id: cursor back to frame 0, control word untouched.
    bank.bind_actor_clip(PLAYER_ANCHOR_TARGET, 5, 15);
    let a = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap();
    assert_eq!(a.anim_id, 5);
    assert_eq!(a.cursor, 0, "the binder zeroes +0x68 on a clip change");
    assert_eq!(a.flags, marked, "the binder never writes +0x62");
}

/// The once-per-frame arbitration: whichever driver gets there first
/// advances the cursor, and the other one does not advance it again.
#[test]
fn a_cross_context_cursor_advances_once_per_frame() {
    let mut bank = PropAnimBank::default();
    bank.bind_actor_clip(PLAYER_ANCHOR_TARGET, 4, 15);
    let step = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().rate;

    // Field frame: the prop-layer tick moves it, the runner's catch-up
    // declines.
    let before = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().cursor;
    bank.tick_anims();
    assert!(!bank.tick_actor_clips_for_frame(), "already ticked");
    assert_eq!(
        bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().cursor - before,
        step
    );

    // Conversation-only frame: the catch-up is the tick.
    let before = bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().cursor;
    assert!(bank.tick_actor_clips_for_frame(), "nothing ticked it yet");
    assert_eq!(
        bank.actor_clip(PLAYER_ANCHOR_TARGET).unwrap().cursor - before,
        step
    );
}

#[test]
fn no_lut_lands_on_ground_plane() {
    let env_tmds = vec![10];
    let placements = vec![placement(Some(0), Some(6), 8)];
    let (draws, _) = resolve_env_draws(&env_tmds, &placements, None);
    assert_eq!(draws[0].world_y, 0);
}

/// One game frame of the anim tick moves the cursor `step * frame step`
/// (`FUN_800204F8` multiplies by `DAT_1F800393`), so short of a wrap a clip
/// ticked once at scale 3 stands where three scale-1 ticks put it.
#[test]
fn a_scaled_tick_is_the_frame_steps_worth_of_unit_ticks() {
    let mut a = PropAnim::cross_context(9, 15);
    let mut b = a;
    for _ in 0..7 {
        a.tick_scaled(3);
        for _ in 0..3 {
            b.tick();
        }
        assert_eq!(a.cursor, b.cursor);
    }
}
