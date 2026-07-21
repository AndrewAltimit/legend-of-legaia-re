//! Unit tests for the field pad-direction remap (`FUN_800467e8`) and the
//! wall-slide direction resolver (`FUN_80046494`), pinned against the exact
//! retail table values decoded from `SCUS_942.54`.

use super::*;

/// A field world with an all-open collision grid.
fn open_field() -> World {
    let mut w = World::new();
    w.mode = SceneMode::Field;
    w.field_collision_grid = vec![0u8; FIELD_GRID_LEN];
    w
}

/// Mark the wall sub-cell covering world `(x, z)` (retail's biased
/// derivation, the same one [`World::field_tile_is_wall`] reads).
fn set_wall(w: &mut World, x: i16, z: i16) {
    let xc = (((x as i32) + 0x3F) >> 6) - 1;
    let zc = ((z as i32) >> 6) + 2;
    let idx = ((xc / 2) & 0x7F) as usize + ((zc >> 1) as usize) * FIELD_GRID_STRIDE;
    let quad = ((zc & 1) << 1 | (xc & 1)) as u32;
    w.field_collision_grid[idx] |= 0x10u8 << quad;
}

// ---- FUN_800467e8 : camera-relative pad-direction ring remap -------------

#[test]
fn remap_identity_when_rot_zero() {
    // rot == 0 short-circuits to the raw mask for every direction.
    for d in [
        0x1000u16, 0x2000, 0x4000, 0x8000, 0x3000, 0x6000, 0x9000, 0xC000,
    ] {
        assert_eq!(World::remap_pad_direction(d, 0), d);
    }
}

#[test]
fn remap_passes_through_when_no_direction() {
    // No direction nibble -> nothing to rotate, even with a live camera.
    assert_eq!(World::remap_pad_direction(0x0001, 3), 0x0001);
    assert_eq!(World::remap_pad_direction(0x0000, 5), 0x0000);
}

#[test]
fn remap_rotates_cardinals_by_two_eighths() {
    // Ring: Z+, Z+X+, X+, X+Z-, Z-, Z-X-, X-, X-Z+.
    // rot=2 (a quarter turn) advances two ring slots.
    assert_eq!(World::remap_pad_direction(0x1000, 2), 0x2000); // Z+ -> X+
    assert_eq!(World::remap_pad_direction(0x2000, 2), 0x4000); // X+ -> Z-
    assert_eq!(World::remap_pad_direction(0x4000, 2), 0x8000); // Z- -> X-
    assert_eq!(World::remap_pad_direction(0x8000, 2), 0x1000); // X- -> Z+ (wrap)
}

#[test]
fn remap_rotates_diagonals_as_ring_entries() {
    // Diagonals are first-class ring slots (the 45-degree remap).
    assert_eq!(World::remap_pad_direction(0x3000, 1), 0x2000); // Z+X+ -> X+
    assert_eq!(World::remap_pad_direction(0x9000, 1), 0x1000); // X-Z+ -> Z+ (wrap)
}

#[test]
fn remap_wraps_full_turn_and_preserves_low_bits() {
    // rot=8 is a full turn: identity direction, low bits untouched.
    assert_eq!(World::remap_pad_direction(0x1000, 8), 0x1000);
    // The direction nibble is rewritten; the rest of the mask survives.
    assert_eq!(World::remap_pad_direction(0x1001, 2), 0x2001);
}

// ---- FUN_80046494 : wall-slide direction resolver ------------------------

#[test]
fn slide_noclip_bit_passes_through() {
    // held & 0x2 (the no-clip pad bit) returns the raw mask untouched.
    let w = open_field();
    assert_eq!(w.resolve_field_slide(0x4002, 100, 100), 0x4002);
}

#[test]
fn slide_diagonals_pass_through() {
    // The four pure diagonals are never slide-resolved.
    let w = open_field();
    for d in [0x9000u16, 0xC000, 0x3000, 0x6000] {
        assert_eq!(w.resolve_field_slide(d, 100, 100), d);
    }
}

#[test]
fn slide_open_direction_returns_bare_mask() {
    // No wall ahead -> the candidate three-point test is clear, so the
    // resolver just re-emits the held direction with no slide.
    let w = open_field();
    assert_eq!(w.resolve_field_slide(0x4000, 1985, 1984), 0x4000);
}

#[test]
fn slide_z_minus_blocked_slides_toward_open_x_minus() {
    // Player at a sub-cell-aligned spot, walking Z- (0x4000). The candidate
    // row is z = 1984 - 62 = 1922. Wall everything at x >= px along that row,
    // leaving the X- side open: the block test trips (centre is a wall) and
    // the perpendicular sweep sums negative -> slide X- (0x8000).
    let (px, pz) = (1985i16, 1984i16);
    let cz = pz - 62;
    let mut w = open_field();
    for x in (px..=px + 160).step_by(16) {
        set_wall(&mut w, x, cz);
    }
    // Preconditions the resolver depends on.
    assert!(w.field_tile_is_wall(px, cz), "candidate centre is a wall");
    assert!(!w.field_tile_is_wall(px - 64, cz), "X- side stays open");
    assert_eq!(w.resolve_field_slide(0x4000, px, pz), 0xC000); // Z- | X-
}

#[test]
fn slide_z_minus_blocked_slides_toward_open_x_plus() {
    // Mirror image: wall the X- side, leave X+ open -> slide X+ (0x2000).
    let (px, pz) = (1985i16, 1984i16);
    let cz = pz - 62;
    let mut w = open_field();
    for x in (px - 160..=px).step_by(16) {
        set_wall(&mut w, x, cz);
    }
    assert!(w.field_tile_is_wall(px, cz), "candidate centre is a wall");
    assert!(!w.field_tile_is_wall(px + 96, cz), "X+ side stays open");
    assert_eq!(w.resolve_field_slide(0x4000, px, pz), 0x6000); // Z- | X+
}

#[test]
fn slide_x_minus_blocked_slides_toward_open_z_minus() {
    // Walking X- (0x8000): the candidate column is x = 1985 - 62 = 1923, and
    // the sweep runs in Z. Wall z >= pz, leave the Z- side open -> the sweep
    // sums negative and the X-travel row picks its Z- slide bit (0x4000).
    let (px, pz) = (1985i16, 1984i16);
    let cx = px - 62;
    let mut w = open_field();
    for z in (pz..=pz + 160).step_by(16) {
        set_wall(&mut w, cx, z);
    }
    assert!(w.field_tile_is_wall(cx, pz), "candidate centre is a wall");
    assert!(!w.field_tile_is_wall(cx, pz - 64), "Z- side stays open");
    assert_eq!(w.resolve_field_slide(0x8000, px, pz), 0xC000); // X- | Z-
}

#[test]
fn slide_symmetric_dead_end_adds_no_slide() {
    // Block the candidate row on BOTH perpendicular sides symmetrically: the
    // sweep sum is zero, so retail's strict `bgez`/`blez` pair slides neither
    // way - only the bare direction bit survives.
    let (px, pz) = (1985i16, 1984i16);
    let cz = pz - 62;
    let mut w = open_field();
    for x in (px - 160..=px + 160).step_by(16) {
        set_wall(&mut w, x, cz);
    }
    assert!(w.field_tile_is_wall(px, cz), "candidate centre is a wall");
    assert_eq!(w.resolve_field_slide(0x4000, px, pz), 0x4000); // no slide bit
}
