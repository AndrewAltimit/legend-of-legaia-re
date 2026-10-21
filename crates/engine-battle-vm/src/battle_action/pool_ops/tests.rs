use super::*;

#[test]
fn flag_scrub_masks_first_seven_slots() {
    let mut words = [0xFFFF_FFFFu32; 8];
    clear_pool_flag_words(&mut words);
    for w in words.iter().take(7) {
        assert_eq!(*w, 0x7cff_ffff);
    }
    // Slot 7 is untouched by the `< 7` loop.
    assert_eq!(words[7], 0xFFFF_FFFF);
}

#[test]
fn flag_scrub_clears_exactly_bits_31_25_24() {
    let mut words = [0x8300_0001u32];
    clear_pool_flag_words(&mut words);
    assert_eq!(words[0], 0x0000_0001);
}

#[test]
fn flag_scrub_truncates_short_slice() {
    let mut words = [0xFFFF_FFFFu32; 3];
    clear_pool_flag_words(&mut words);
    assert!(words.iter().all(|w| *w == 0x7cff_ffff));
}

#[test]
fn formation_within_frame_only_recenters() {
    // Span 400 in X (< 0x800), 0 in Z: no normalise pass, just recentre.
    let mut pos = [
        FormationPos { x: 100, z: 0 },
        FormationPos { x: 500, z: 0 },
        FormationPos { x: 300, z: 0 },
    ];
    let alive = [true, true, true];
    let (mut fx, mut fz) = (0i32, 0i32);
    normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
    // centroid X = (500 + 100) >> 1 = 300.
    assert_eq!(pos[0].x, -200);
    assert_eq!(pos[1].x, 200);
    assert_eq!(pos[2].x, 0);
    assert_eq!(fx, 300);
    assert_eq!(fz, 0);
}

#[test]
fn formation_wide_x_is_squashed_then_centered() {
    // X span 4000 (> 0x800). span = 4000, scale = (x << 11) / 4000.
    let mut pos = [
        FormationPos { x: -2000, z: 0 },
        FormationPos { x: 2000, z: 0 },
        FormationPos { x: 0, z: 0 },
    ];
    let alive = [true, true, true];
    let (mut fx, mut fz) = (0i32, 0i32);
    normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
    // After scale: -2000 -> (-2000<<11)/4000 = -1024; 2000 -> 1024; 0 -> 0.
    // Then centroid = (1024 + -1024) >> 1 = 0, so no shift.
    assert_eq!(pos[0].x, -1024);
    assert_eq!(pos[1].x, 1024);
    assert_eq!(pos[2].x, 0);
    assert_eq!(fx, 0);
}

#[test]
fn formation_skips_dead_monster_slots() {
    // Slot 3 (a monster) is dead and must not move the extents.
    let mut pos = [
        FormationPos { x: 0, z: 0 },
        FormationPos { x: 0, z: 0 },
        FormationPos { x: 0, z: 0 },
        FormationPos { x: 9000, z: 0 }, // dead - ignored
    ];
    let alive = [true, true, true, false];
    let (mut fx, mut fz) = (0i32, 0i32);
    normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
    // Extents only over slots 0..2 (all x=0): centroid 0, nothing moves.
    assert_eq!(pos[0].x, 0);
    assert_eq!(pos[3].x, 9000);
    assert_eq!(fx, 0);
}

#[test]
fn target_cycle_mid_ring() {
    // count=3, queue[1]=10 (wrap slot), queue[2..]=[20,30,40].
    // active_target=20 matches at uvar2=2 -> idx=1 (idx = match_pos - 1).
    let queue = [3u8, 10, 20, 30, 40];
    // Next: idx==count-1? 1==2 no -> queue[idx+2] = queue[3] = 30.
    assert_eq!(cycle_attack_target(&queue, 20, TargetCycle::Next), 30);
    // Prev: idx==0? no -> queue[idx] = queue[1] = 10.
    assert_eq!(cycle_attack_target(&queue, 20, TargetCycle::Prev), 10);
}

#[test]
fn target_cycle_wraps() {
    let queue = [3u8, 10, 20, 30, 40];
    // active_target=30 matches at uvar2=3 (=count) -> idx=2=count-1.
    // Next wraps to queue[1]=10.
    assert_eq!(cycle_attack_target(&queue, 30, TargetCycle::Next), 10);
    // active_target=10 matches at uvar2=1 -> idx=0. Prev wraps to
    // queue[count]=queue[3]=30.
    assert_eq!(cycle_attack_target(&queue, 10, TargetCycle::Prev), 30);
}

#[test]
fn redirect_skips_when_target_alive() {
    let q = RedirectQuery {
        target_slot: 3,
        category: 3,
        param0: 0,
    };
    let out = redirect_dead_target(q, 1, 2, || 0, |_| true, |_| 0);
    assert_eq!(out, None);
}

#[test]
fn redirect_attack_rerolls_to_living_enemy() {
    let q = RedirectQuery {
        target_slot: 3, // dead enemy
        category: 3,
        param0: 0,
    };
    // monster_count=3 -> slots 3,4,5. Slot 3 dead, 4 alive.
    let mut rolls = [0i32, 1i32].into_iter();
    let out = redirect_dead_target(
        q,
        1,
        3,
        move || rolls.next().unwrap(),
        |slot| slot == 4, // only slot 4 alive
        |_| 0,
    );
    // roll 0 -> slot 3 (dead, retry); roll 1 -> slot 4 (alive).
    assert_eq!(out, Some(4));
}

#[test]
fn redirect_magic_offensive_at_party_target_is_left_alone() {
    // Magic, low-tier (< 0xA), target a dead party slot -> no reroll.
    let q = RedirectQuery {
        target_slot: 1,
        category: 2,
        param0: 0x10,
    };
    let out = redirect_dead_target(q, 3, 2, || 0, |_| false, |_| 5);
    assert_eq!(out, None);
}

#[test]
fn redirect_magic_status_spell_rerolls() {
    // Magic, high-tier (>= 0xA) -> reroll on the party side.
    let q = RedirectQuery {
        target_slot: 1, // dead party slot
        category: 2,
        param0: 0x20,
    };
    let out = redirect_dead_target(q, 3, 2, || 2, |slot| slot == 2, |_| 0xa);
    assert_eq!(out, Some(2));
}

#[test]
fn redirect_item_only_special_ids() {
    let dead = |_: u8| false;
    let base = RedirectQuery {
        target_slot: 4,
        category: 1,
        param0: 0x01,
    };
    // Ordinary item id, dead enemy target -> no reroll.
    assert_eq!(redirect_dead_target(base, 1, 3, || 0, dead, |_| 0), None);
    // Special id 0xFE -> reroll on the enemy side. monster_count=3 covers
    // slots 3,4,5; slot 4 (target) dead, slot 5 alive. rng 2 -> slot 5.
    let q = RedirectQuery {
        param0: 0xfe,
        ..base
    };
    assert_eq!(
        redirect_dead_target(q, 1, 3, || 2, |s| s == 5, |_| 0),
        Some(5)
    );
}

/// Build an 8-slot pool: party 0..2 alive, monsters at the given z's.
fn pool_with_monster_z(zs: [i16; 4], alive: [bool; 4], target: u8) -> Vec<PoolActor> {
    let mut pool = vec![PoolActor::default(); 8];
    for p in pool.iter_mut().take(3) {
        p.alive = true;
    }
    pool[0].target_slot = target;
    for (i, (&z, &a)) in zs.iter().zip(alive.iter()).enumerate() {
        pool[3 + i] = PoolActor {
            alive: a,
            x: 0,
            z,
            target_slot: 0,
            status: 0,
        };
    }
    pool
}

// Deterministic stand-in for the bearing: returns `target.z & 0xFFF`, which
// lets the queue test predict the relative ordering by hand.
fn z_bearing(tz: i16, _tx: i16, _az: i16, _ax: i16) -> u16 {
    (tz as u16) & 0xfff
}

#[test]
fn attack_queue_orders_alternates_by_angular_distance() {
    // Monsters slots 3,4,5,6 with z = 0x100,0x050,0x200,0x300; actor targets
    // slot 3. ref_ang = (0x100 + 0x800) & 0xFFF = 0x900.
    //   slot3 rel=0 (excluded: it is the current target/wrap)
    //   slot4 rel=0xF50, slot5 rel=0x100, slot6 rel=0x200
    // Nearest-first over {4,5,6}: 5 (0x100), 6 (0x200), 4 (0xF50).
    let pool = pool_with_monster_z([0x100, 0x050, 0x200, 0x300], [true; 4], 3);
    let q = build_attack_target_queue(&pool, 0, z_bearing);
    assert_eq!(q.count, 4);
    assert_eq!(q.wrap_slot, 3);
    assert_eq!(q.ordered, [5, 6, 4]);
}

#[test]
fn attack_queue_composes_with_cycle() {
    // The ring built above, fed straight into cycle_attack_target.
    let pool = pool_with_monster_z([0x100, 0x050, 0x200, 0x300], [true; 4], 3);
    let q = build_attack_target_queue(&pool, 0, z_bearing);
    let ring = [
        q.count,
        q.wrap_slot,
        q.ordered[0],
        q.ordered[1],
        q.ordered[2],
    ];
    // From the current target 3: Next -> 5, and Prev wraps to 4.
    assert_eq!(cycle_attack_target(&ring, 3, TargetCycle::Next), 5);
    assert_eq!(cycle_attack_target(&ring, 3, TargetCycle::Prev), 4);
}

#[test]
fn attack_queue_skips_dead_monsters() {
    // Only the current target (slot 3) is alive; no alternates exist.
    let pool = pool_with_monster_z([0x100, 0, 0, 0], [true, false, false, false], 3);
    let q = build_attack_target_queue(&pool, 0, z_bearing);
    assert_eq!(q.count, 1);
    assert_eq!(q.wrap_slot, 3);
    assert_eq!(q.ordered, [0, 0, 0]);
}

#[test]
fn first_live_monster_scans_slots_3_to_6() {
    let mut pool = vec![PoolActor::default(); 8];
    pool[5].alive = true;
    assert_eq!(first_live_monster_slot(&pool), 5);
    // None alive -> fall-through value 7.
    let empty = vec![PoolActor::default(); 8];
    assert_eq!(first_live_monster_slot(&empty), 7);
    // Slot 7 alive is out of the 3..=6 scan, so still 7.
    let mut only7 = vec![PoolActor::default(); 8];
    only7[7].alive = true;
    assert_eq!(first_live_monster_slot(&only7), 7);
}

#[test]
fn selectable_scans_apply_all_three_predicates() {
    let mut pool = vec![PoolActor::default(); 8];
    for p in pool.iter_mut() {
        p.alive = true;
    }
    // Slot 1 afflicted (0x004 in the 0xF84 mask); slot 2 seats the
    // AI-controlled companion (roster char id 4); slot 3 is the first
    // fully-selectable one.
    pool[1].status = 0x004;
    let roster = [1u8, 2, AI_COMPANION_CHAR_ID, 3, 3, 3, 3, 3];
    assert_eq!(first_selectable_target(&pool, &roster, 6), 0);
    pool[0].alive = false; // slot 0 out -> skip 1 (ailment), 2 (AI) -> 3.
    assert_eq!(first_selectable_target(&pool, &roster, 6), 3);
    // count == 0 returns the 0 seed.
    assert_eq!(first_selectable_target(&pool, &roster, 0), 0);
}

#[test]
fn next_selectable_starts_after_current() {
    let mut pool = vec![PoolActor::default(); 8];
    for p in pool.iter_mut() {
        p.alive = true;
    }
    let roster = [1u8; 8];
    // From current index 2, the next is 3.
    assert_eq!(next_selectable_actor(&pool, &roster, 6, 2), 3);
    // current+1 already >= count -> returns current+1 unscanned.
    assert_eq!(next_selectable_actor(&pool, &roster, 4, 5), 6);
    // No qualifying slot after current -> returns count.
    pool[3].alive = false;
    pool[4].status = 0x080;
    let roster2 = [1u8, 1, 1, 1, 1, AI_COMPANION_CHAR_ID, 1, 1];
    assert_eq!(next_selectable_actor(&pool, &roster2, 6, 2), 6);
}

#[test]
fn bearing_axis_cases_are_exact() {
    // atan_lut[0] must be 0; the axis cases never touch the LUT interior.
    let lut = vec![0i16; 0x801];
    // dz = p2z - p1z, dx = p2x - p1x.
    // +Z (dz>0, dx=0) -> 0.
    assert_eq!(bearing_12bit(&lut, 0, 0, 100, 0), 0);
    // +X (dz=0, dx>0) -> 0x400.
    assert_eq!(bearing_12bit(&lut, 0, 0, 0, 100), 0x400);
    // -Z (dz<0, dx=0) -> 0x800.
    assert_eq!(bearing_12bit(&lut, 0, 0, -100, 0), 0x800);
    // -X (dz=0, dx<0) -> 0xC00.
    assert_eq!(bearing_12bit(&lut, 0, 0, 0, -100), 0xc00);
}

#[test]
fn approx_lut_pins_the_two_values_the_quadrant_algebra_needs() {
    let lut = approx_arctan_lut();
    assert_eq!(lut.len(), ARCTAN_LUT_LEN);
    assert_eq!(lut[0], 0, "the zero entry the axis cases return directly");
    assert_eq!(lut[0x800], 0x200, "45 degrees in 12-bit turn units");
    // Monotone non-decreasing: an arctan table that dipped would break the
    // nearest-alternate ordering the target ring is built on.
    assert!(lut.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn approx_bearing_names_the_four_axes() {
    // The heading convention, stated as headings rather than as table
    // entries: +Z is 0, and the circle runs +Z -> +X -> -Z -> -X.
    assert_eq!(bearing_12bit_approx(0, 0, 100, 0), 0x000);
    assert_eq!(bearing_12bit_approx(0, 0, 0, 100), 0x400);
    assert_eq!(bearing_12bit_approx(0, 0, -100, 0), 0x800);
    assert_eq!(bearing_12bit_approx(0, 0, 0, -100), 0xc00);
    // And the four diagonals land on the octant midpoints.
    assert_eq!(bearing_12bit_approx(0, 0, 100, 100), 0x200);
    assert_eq!(bearing_12bit_approx(0, 0, -100, 100), 0x600);
    assert_eq!(bearing_12bit_approx(0, 0, -100, -100), 0xa00);
    assert_eq!(bearing_12bit_approx(0, 0, 100, -100), 0xe00);
}

#[test]
fn approx_bearing_orders_the_retail_monster_seats() {
    // The authored 4-monster row (battle_seats::MONSTER_SEATS[3]) seen
    // from the lead party seat: the ring must order the alternates by
    // angular distance, so the two flanks come out either side of the
    // centre pair rather than in slot order.
    let seats = [(-900i16, 900i16), (-300, 800), (300, 800), (900, 900)];
    let mut pool = vec![PoolActor::default(); 8];
    pool[0] = PoolActor {
        alive: true,
        x: 0,
        z: -825,
        target_slot: 4, // currently aimed at the left-of-centre monster
        status: 0,
    };
    for (i, (x, z)) in seats.iter().enumerate() {
        pool[3 + i] = PoolActor {
            alive: true,
            x: *x,
            z: *z,
            target_slot: 0,
            status: 0,
        };
    }
    let q = build_attack_target_queue(&pool, 0, bearing_12bit_approx);
    assert_eq!(q.count, 4);
    assert_eq!(q.wrap_slot, 4);
    // From the left-of-centre monster, sweeping one way: right-of-centre,
    // then the far right flank, then the far left flank.
    assert_eq!(q.ordered, [5, 6, 3]);
    let ring = [
        q.count,
        q.wrap_slot,
        q.ordered[0],
        q.ordered[1],
        q.ordered[2],
    ];
    assert_eq!(cycle_attack_target(&ring, 4, TargetCycle::Next), 5);
    assert_eq!(cycle_attack_target(&ring, 4, TargetCycle::Prev), 3);
}

#[test]
fn bearing_diagonals_hit_lut_end() {
    // Synthetic LUT: linear ramp, lut[i] = i/4, so lut[0x800] = 0x200 (45deg).
    let lut: Vec<i16> = (0..=0x800).map(|i| (i / 4) as i16).collect();
    // dz == dx > 0, quad 0, dz>=dx path: idx = (dx<<11)/dz = 0x800 -> 0x200.
    assert_eq!(bearing_12bit(&lut, 0, 0, 100, 100), 0x200);
    // quad 0, dz < dx: 0x400 - lut[(dz<<11)/dx]. dz=50,dx=100 -> idx=0x400,
    // lut=0x100 -> 0x300.
    assert_eq!(bearing_12bit(&lut, 0, 0, 50, 100), 0x300);
    // quad 1 (dx<0), dz==dx magnitude: 0x1000 - lut[0x800] = 0xE00.
    assert_eq!(bearing_12bit(&lut, 0, 0, 100, -100), 0xe00);
    // quad 3 (both negative), dz>=dx, dx!=0: lut[idx] + 0x800.
    // dz=100,dx=50 -> idx=(50<<11)/100=0x400, lut=0x100 -> 0x900.
    assert_eq!(bearing_12bit(&lut, 0, 0, -100, -50), 0x900);
}

/// The action seed's recentre puts a melee's target on the stage origin and
/// translates everyone else with it: retail's Cort fight, Noa attacking from
/// the three-on-one formation, reads `(0,-1625) (600,-1575) (-600,-1575)
/// (0,0)` on the pass after the seed (`autorun` position capture of
/// `cort_evolved_battle_first_menu`).
#[test]
fn an_attack_seed_recentres_on_its_target() {
    let mut pos = [
        FormationPos { x: 0, z: -812 },
        FormationPos { x: 600, z: -762 },
        FormationPos { x: -600, z: -762 },
        FormationPos { x: 0, z: 813 },
    ];
    let alive = [true, true, true, true];
    let (mut fx, mut fz) = (0i32, 0i32);
    seed_formation_recentre(&mut pos, &alive, true, 3, &mut fx, &mut fz);
    assert_eq!(
        pos.map(|p| (p.x, p.z)),
        [(0, -1625), (600, -1575), (-600, -1575), (0, 0)]
    );
    assert_eq!((fx, fz), (0, 813));
}

/// A non-Attack seed frames the whole formation: the recentre is the
/// round-start one.
#[test]
fn a_non_attack_seed_recentres_like_the_round_start() {
    let start = [
        FormationPos { x: 0, z: -812 },
        FormationPos { x: 600, z: -762 },
        FormationPos { x: -600, z: -762 },
        FormationPos { x: 300, z: 1300 },
    ];
    let alive = [true, true, true, true];
    let (mut a, mut b) = (start, start);
    let (mut fx, mut fz) = (0i32, 0i32);
    seed_formation_recentre(&mut a, &alive, false, 3, &mut fx, &mut fz);
    let (mut gx, mut gz) = (0i32, 0i32);
    normalize_formation_span(&mut b, &alive, &mut gx, &mut gz);
    assert_eq!(a, b);
    assert_eq!((fx, fz), (gx, gz));
}

/// A group target frames its whole side: `9` boxes every live monster seat.
#[test]
fn an_all_monster_attack_frames_the_monster_side() {
    let mut pos = [
        FormationPos { x: 0, z: -800 },
        FormationPos::default(),
        FormationPos::default(),
        FormationPos { x: -300, z: 800 },
        FormationPos { x: 300, z: 1000 },
    ];
    let alive = [true, false, false, true, true];
    let (mut fx, mut fz) = (0i32, 0i32);
    seed_formation_recentre(&mut pos, &alive, true, 9, &mut fx, &mut fz);
    // Box x -300..300, z 800..1000: centroid (0, 900).
    assert_eq!((pos[3].x, pos[3].z), (-300, -100));
    assert_eq!((pos[0].x, pos[0].z), (0, -1700));
}
