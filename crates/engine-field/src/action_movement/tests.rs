use super::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// A synthetic field: 128-unit tiles, each with a floor height (Y down) or a
/// wall.
#[derive(Default)]
struct Tiles {
    floors: HashMap<(i32, i32), i32>,
    walls: Vec<(i32, i32)>,
}

impl Tiles {
    fn tile(x: i32, z: i32) -> (i32, i32) {
        (x.div_euclid(128), z.div_euclid(128))
    }
}

impl ActionTerrain for Tiles {
    fn step_blocked(&mut self, x: i16, z: i16, dir: usize) -> bool {
        let (dx, dz) = [(0, -1), (-1, 0), (0, 1), (1, 0)][dir];
        let t = Self::tile(x as i32 + dx, z as i32 + dz);
        self.walls.contains(&t)
    }
    fn floor_y(&mut self, x: i32, z: i32) -> i32 {
        *self.floors.get(&Self::tile(x, z)).unwrap_or(&0)
    }
}

fn idle() -> ActionInput {
    ActionInput::default()
}

fn right() -> ActionInput {
    ActionInput {
        stick: (STICK_FULL, 0),
        ..ActionInput::default()
    }
}

fn press(mut i: ActionInput) -> ActionInput {
    i.jump_pressed = true;
    i.jump_held = true;
    i
}

fn hold(mut i: ActionInput) -> ActionInput {
    i.jump_held = true;
    i
}

/// Run a held jump on flat ground: the apex and the frame it lands on.
fn held_jump(p: &ActionParams) -> (i32, usize) {
    let mut t = Tiles::default();
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(p, &press(idle()), &mut t);
    let mut apex = 0;
    for f in 1..200 {
        let s = b.step(p, &hold(idle()), &mut t);
        apex = apex.min(b.unit_pos().1);
        if s.landed {
            return (apex, f);
        }
    }
    panic!("never landed");
}

#[test]
fn a_held_jump_peaks_once_and_lands_on_the_floor() {
    let p = ActionParams::default();
    let (apex, frames) = held_jump(&p);
    // v0 = 27, g = 2.5: v0^2 / 2g = 145.8 units of rise in the continuum;
    // gravity lands before the first move, so the stepped arc peaks lower.
    assert!((-146..=-125).contains(&apex), "apex {apex}");
    assert!((16..=26).contains(&frames), "airtime {frames}");
    // The same input is the same arc, frame for frame.
    assert_eq!(held_jump(&p), (apex, frames));
}

#[test]
fn letting_go_early_jumps_lower() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(&p, &press(idle()), &mut t);
    let mut apex = 0;
    for _ in 0..80 {
        b.step(&p, &idle(), &mut t);
        apex = apex.min(b.unit_pos().1);
    }
    assert!(b.grounded);
    assert!(apex > held_jump(&p).0 / 2, "short hop {apex}");
}

#[test]
fn a_double_jump_goes_higher_and_only_once() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(&p, &press(idle()), &mut t);
    let mut apex = 0;
    let mut jumps = 1;
    for f in 1..120 {
        // Press again at the first apex, and once more after that.
        let i = if f == 11 || f == 20 {
            press(idle())
        } else {
            hold(idle())
        };
        let s = b.step(&p, &i, &mut t);
        jumps += s.jumped as u32;
        apex = apex.min(b.unit_pos().1);
        if s.landed {
            break;
        }
    }
    assert_eq!(jumps, 2, "one ground jump plus one air jump");
    assert!(apex < held_jump(&p).0 - 80, "double-jump apex {apex}");
}

#[test]
fn walls_stop_the_body_at_the_tile_edge() {
    let p = ActionParams::default();
    let mut t = Tiles {
        walls: vec![(2, 0)],
        ..Tiles::default()
    };
    let mut b = ActionBody::at(64, 0, 64, 0);
    let mut blocked = false;
    for _ in 0..60 {
        let s = b.step(&p, &right(), &mut t);
        blocked |= s.blocked.0;
        assert!(
            b.unit_pos().0 < 256,
            "entered the wall at {:?}",
            b.unit_pos()
        );
    }
    assert!(blocked);
    assert_eq!(b.unit_pos().0, 255);
    // A jump does not carry the body over a wall either.
    for f in 0..60 {
        let i = if f == 0 {
            press(right())
        } else {
            hold(right())
        };
        b.step(&p, &i, &mut t);
        assert!(b.unit_pos().0 < 256);
    }
}

#[test]
fn a_ledge_blocks_the_walk_and_a_jump_climbs_it() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    t.floors.insert((2, 0), -100);
    let mut b = ActionBody::at(64, 0, 64, 0);
    for _ in 0..40 {
        b.step(&p, &right(), &mut t);
    }
    assert_eq!(b.unit_pos().0, 255, "the 100-unit ledge refuses a walk");
    assert!(b.grounded);
    let mut landed_on_top = false;
    for f in 0..60 {
        // Steer over the lip, then let go and settle onto the top.
        let steer = if b.unit_pos().0 < 270 {
            right()
        } else {
            idle()
        };
        let i = if f == 0 { press(steer) } else { hold(steer) };
        let s = b.step(&p, &i, &mut t);
        if s.landed && b.unit_pos().1 == -100 {
            landed_on_top = true;
        }
    }
    assert!(landed_on_top, "ended at {:?}", b.unit_pos());
    assert!((256..384).contains(&b.unit_pos().0));
}

#[test]
fn a_cliff_taller_than_both_jumps_stays_out_of_reach() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    t.floors.insert((2, 0), -600);
    let mut b = ActionBody::at(200, 0, 64, 0);
    for f in 0..120 {
        let i = if f == 0 || f == 11 {
            press(right())
        } else {
            hold(right())
        };
        b.step(&p, &i, &mut t);
        assert!(b.unit_pos().0 < 256);
    }
}

#[test]
fn stairs_are_walked_and_a_drop_is_fallen() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    t.floors.insert((1, 0), -32);
    t.floors.insert((2, 0), -64);
    for tile in 3..12 {
        t.floors.insert((tile, 0), 200);
    }
    let mut b = ActionBody::at(64, 0, 64, 0);
    let mut fell = false;
    for _ in 0..60 {
        let s = b.step(&p, &right(), &mut t);
        fell |= s.anim == ActionAnim::Fall;
        if b.unit_pos().0 == 300 {
            assert_eq!(b.unit_pos().1, -64, "climbed both stairs");
        }
    }
    assert!(fell, "the 264-unit drop is a fall, not a snap");
    assert!(b.grounded);
    assert_eq!(b.unit_pos().1, 200);
}

/// Walk off a ledge and press jump `late` frames after the floor goes.
fn jump_after_leaving(late: u32) -> (i32, ActionBody) {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    t.floors.insert((1, 0), 400);
    let mut b = ActionBody::at(120, 0, 64, 0);
    let mut left_at = None;
    for f in 0..80u32 {
        let jump = left_at.is_some_and(|l| f == l + late);
        let i = if jump { press(right()) } else { right() };
        let vy_before = b.vy;
        let s = b.step(&p, &i, &mut t);
        if s.jumped {
            return (vy_before, b);
        }
        if left_at.is_none() && !b.grounded {
            left_at = Some(f);
        }
    }
    (0, b)
}

#[test]
fn coyote_time_turns_a_late_press_into_a_ground_jump() {
    let p = ActionParams::default();
    let (_, b) = jump_after_leaving(3);
    assert_eq!(
        b.air_jumps_used, 0,
        "inside the coyote window: a ground jump"
    );
    let (_, b) = jump_after_leaving(u32::from(p.coyote_frames) + 2);
    assert_eq!(b.air_jumps_used, 1, "past it: the air jump");
}

#[test]
fn a_press_just_before_landing_is_buffered() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(&p, &press(idle()), &mut t);
    // Spend the air jump so the second press can only be buffered.
    b.step(&p, &press(idle()), &mut t);
    let mut land = 0;
    for _ in 0..80 {
        land += 1;
        if b.step(&p, &hold(idle()), &mut t).landed {
            break;
        }
    }
    // Re-run, pressing three frames before the landing.
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(&p, &press(idle()), &mut t);
    b.step(&p, &press(idle()), &mut t);
    let mut jumped_after_landing = false;
    let mut landed = false;
    for f in 0..land + 4 {
        let i = if f + 3 == land {
            press(idle())
        } else {
            hold(idle())
        };
        let s = b.step(&p, &i, &mut t);
        if landed && s.jumped {
            jumped_after_landing = true;
        }
        landed |= s.landed;
    }
    assert!(landed && jumped_after_landing);
    // Pressed too early, the buffer has run out by the landing.
    let mut b = ActionBody::at(64, 0, 64, 0);
    b.step(&p, &press(idle()), &mut t);
    b.step(&p, &press(idle()), &mut t);
    let mut rejumped = false;
    let mut landed = false;
    for f in 0..land + 4 {
        let i = if f + 12 == land {
            press(idle())
        } else {
            hold(idle())
        };
        let s = b.step(&p, &i, &mut t);
        rejumped |= landed && s.jumped;
        landed |= s.landed;
    }
    assert!(landed && !rejumped);
}

#[test]
fn a_dash_bursts_then_cools_down() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    let mut b = ActionBody::at(64, 0, 64, 0x400);
    let dash = ActionInput {
        dash_pressed: true,
        ..ActionInput::default()
    };
    assert!(b.step(&p, &dash, &mut t).dashed, "dashes along the facing");
    for _ in 1..p.dash_frames {
        assert_eq!(b.step(&p, &idle(), &mut t).anim, ActionAnim::Dash);
    }
    let travelled = b.unit_pos().0 - 64;
    assert!(
        travelled >= 36 * (i32::from(p.dash_frames) - 1),
        "{travelled}"
    );
    assert!(!b.step(&p, &dash, &mut t).dashed, "still cooling down");
}

#[test]
fn the_run_reaches_top_speed_and_a_reversal_turns_fast() {
    let p = ActionParams::default();
    let mut t = Tiles::default();
    let mut b = ActionBody::at(2000, 0, 64, 0);
    for _ in 0..10 {
        b.step(&p, &right(), &mut t);
    }
    assert_eq!(b.vel.0, p.run_speed);
    let left = ActionInput {
        stick: (-STICK_FULL, 0),
        ..ActionInput::default()
    };
    let mut frames = 0;
    while b.vel.0 > 0 {
        b.step(&p, &left, &mut t);
        frames += 1;
    }
    assert!(frames <= 3, "reversal took {frames} frames");
    assert_eq!(b.heading, 0xC00, "facing snaps to the stick");
}

#[test]
fn headings_and_roots_are_exact() {
    assert_eq!(heading_of(0, 1), 0);
    assert_eq!(heading_of(1, 0), 0x400);
    assert_eq!(heading_of(0, -1), 0x800);
    assert_eq!(heading_of(-1, 0), 0xC00);
    assert_eq!(heading_of(5, 5), 0x200);
    assert_eq!(heading_of(-5, 5), 0xE00);
    for n in [0u64, 1, 2, 3, 4, 15, 16, 17, 1 << 40, u32::MAX as u64 * 7] {
        let r = isqrt(n);
        assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
    }
}

#[test]
fn a_scripted_input_stream_replays_bit_for_bit() {
    fn run() -> u64 {
        let p = ActionParams::default();
        let mut t = Tiles {
            walls: vec![(4, 2), (0, 3)],
            ..Tiles::default()
        };
        t.floors.insert((2, 1), -100);
        t.floors.insert((3, 1), -64);
        t.floors.insert((1, 2), 150);
        let mut b = ActionBody::at(64, 0, 64, 0);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut seed = 0x1234_5678u32;
        let mut held = false;
        for f in 0..600u32 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let r = seed >> 16;
            let press_jump = r.is_multiple_of(23);
            held = (held || press_jump) && !r.is_multiple_of(7);
            let i = ActionInput {
                stick: (((r & 0xFF) as i32) - 128, (((r >> 8) & 0xFF) as i32) - 128),
                camera_azimuth: (f * 7) as u16,
                jump_pressed: press_jump,
                jump_held: held,
                dash_pressed: r.is_multiple_of(41),
                sprint_held: r & 0x100 != 0,
            };
            let s = b.step(&p, &i, &mut t);
            b.hash(&mut h);
            s.anim.hash(&mut h);
        }
        h.finish()
    }
    assert_eq!(run(), run());
}
