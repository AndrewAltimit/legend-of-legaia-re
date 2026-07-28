//! Battle **on-screen test** - re-anchor a battle sprite on its seat actor,
//! project its square billboard box, and report whether the box overlaps the
//! screen horizontally.
//!
//! PORT: FUN_8005126C
//!
//! NOT WIRED: the two halves of the pass want state this crate does not hold
//! and a consumer the port does not have.
//!
//! * **No seat table.** The re-anchor step reads the 8-slot battle-actor
//!   pointer table `&DAT_801C9370` at the sprite's own seat index
//!   (`+0x5A`) and copies that actor's position `SVECTOR` verbatim. The pool
//!   lives in `legaia_engine_core`'s battle scene, so [`battle_actor_on_screen`]
//!   takes the seat position as an argument instead of resolving it, and
//!   nothing in the engine produces the pair.
//! * **Retail has no consumer for the verdict, and that is now settled.** The
//!   port draws every loaded body every frame - no frustum cull, no draw
//!   distance (see `docs/subsystems/renderer.md`, "No distance culling") - and
//!   retail does the same, because nothing runs this test. A sweep of
//!   `SCUS_942.54`, every based overlay image and the raw bytes of every
//!   extracted `PROT.DAT` entry finds no reference to `0x8005126C` in any
//!   form: no literal address word (so it is in no dispatch table and no actor
//!   template), no `jal`, no `j`, no PC-relative branch, and no `lui`+`addiu`
//!   materialisation. It is a linked-but-unreached entry point - see
//!   `docs/reference/functions/battle.md` § Unreferenced SCUS entry points and
//!   `docs/tooling/address-reference-scan.md`. So the earlier framing ("the
//!   caller has to come first") had no answer to wait for; there is no pass to
//!   wire this into, and a cull built on it would be an invention rather than
//!   a port.
//!
//! REF: FUN_800195a8 - the billboard projector, ported as
//! [`crate::billboard::project_billboard`]; this pass is one of its riders.
//!
//! # The test is horizontal only
//!
//! The four projected corners come back through out-pointers as packed SXY
//! words and the pass reads exactly two of them with `lh` - the **X** of
//! corner 0 (left) and corner 1 (right). Nothing reads a Y at all, so an
//! actor a full screen above or below the viewport still passes. That is
//! consistent with what the box is: `half_w == half_h == actor[+0x58]`, both
//! corners share the plane's view Z and there is no in-plane spin, so
//! corners 2/3 carry the same two X values as 0/1 and the top edge is the
//! whole quad's horizontal extent.
//!
//! # The two rejects
//!
//! Read off `0x800512E0..0x8005132C`, where both branch pairs put the
//! interesting case in the delay slot:
//!
//! 1. `slti 0x141` on each X, rejecting only when **both** are `>= 0x141`
//!    (the box is entirely right of the screen).
//! 2. `bgez` / `bltz` on the same pair, rejecting only when **both** are
//!    `< 0` (entirely left of it).
//!
//! Everything else is accepted, so the composite is exactly "the span
//! `[x0, x1]` intersects `[0, 0x140]`" - 321 accepted columns for a 320-wide
//! frame, the right edge inclusive.
//!
//! Source: `ghidra/scripts/funcs/8005126c.txt` (disassembly).

use crate::billboard::{BillboardCorners, project_billboard};
use crate::gte::{GteMat3, GteVec3};

/// Exclusive right bound of the accepted screen-X band: retail's `slti 0x141`
/// accepts `x <= 0x140` (320), so a 320-wide frame accepts its right edge.
pub const SCREEN_X_LIMIT: i16 = 0x141;

/// Whether a projected horizontal span overlaps the screen band.
///
/// `left` / `right` are the screen X of the box's corner 0 and corner 1 - the
/// two halfwords retail reads back with `lh`. Rejects only the two
/// entirely-off cases; the order of the pair does not matter, so a mirrored
/// box (negative half-width) behaves the same.
pub fn span_overlaps_screen_x(left: i16, right: i16) -> bool {
    if left >= SCREEN_X_LIMIT && right >= SCREEN_X_LIMIT {
        return false;
    }
    if left < 0 && right < 0 {
        return false;
    }
    true
}

/// What one on-screen test resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleActorOnScreen {
    /// The position the pass copies onto the sprite (`actor[+0x14..0x1B]`),
    /// taken verbatim from the seat actor's `+0x3C` `SVECTOR`. The copy is a
    /// side effect of the test, not a read of it - the sprite is re-anchored
    /// on its seat every time the question is asked.
    pub position: (i16, i16, i16),
    /// The projected billboard box.
    pub corners: BillboardCorners,
    /// The routine's `1` / `0` return.
    pub on_screen: bool,
}

/// Re-anchor a battle sprite on its seat actor and test its billboard box
/// against the screen band.
///
/// PORT: FUN_8005126C - see the module docs for the per-step mapping.
///
/// `seat_position` is the `SVECTOR` at `+0x3C` of `(&DAT_801C9370)[actor +
/// 0x5A]`; `half_size` is `actor[+0x58]`, which retail passes as **both** the
/// projector's half-width and half-height. The camera arguments and
/// `ot_shift` are [`project_billboard`]'s, unchanged - retail passes the
/// ambient GTE state and angle `0`.
#[allow(clippy::too_many_arguments)]
pub fn battle_actor_on_screen(
    rot: &GteMat3,
    trans: GteVec3,
    seat_position: (i16, i16, i16),
    half_size: i16,
    h: i32,
    ofx: i32,
    ofy: i32,
    ot_shift: u32,
) -> BattleActorOnScreen {
    let (x, y, z) = seat_position;
    let corners = project_billboard(
        rot,
        trans,
        GteVec3::new(x.into(), y.into(), z.into()),
        half_size,
        half_size,
        0,
        h,
        ofx,
        ofy,
        ot_shift,
    );
    let on_screen = span_overlaps_screen_x(corners.xy[0].0, corners.xy[1].0);
    BattleActorOnScreen {
        position: seat_position,
        corners,
        on_screen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Identity camera, `H = 320`, screen centre at `(160, 120)` - the
    /// ambient projection the battle path runs the test under.
    fn on_screen_at(x: i16, y: i16, z: i16, half: i16) -> BattleActorOnScreen {
        battle_actor_on_screen(
            &GteMat3::IDENTITY,
            GteVec3::default(),
            (x, y, z),
            half,
            320,
            160,
            120,
            0,
        )
    }

    #[test]
    fn span_bounds_are_exactly_the_two_rejects() {
        // Right bound: 0x140 is the last accepted column, 0x141 the first
        // rejected one - and only when BOTH corners are past it.
        assert!(span_overlaps_screen_x(0x140, 0x140));
        assert!(!span_overlaps_screen_x(0x141, 0x141));
        assert!(span_overlaps_screen_x(0x100, 0x141));
        assert!(span_overlaps_screen_x(0x141, 0x100));

        // Left bound: -1 alone is not a reject, -1 on both is.
        assert!(span_overlaps_screen_x(-1, 0));
        assert!(!span_overlaps_screen_x(-1, -1));
        assert!(span_overlaps_screen_x(0, 0));

        // A box that spans the whole screen and then some is accepted.
        assert!(span_overlaps_screen_x(-0x400, 0x3ff));
    }

    #[test]
    fn a_box_straddling_either_edge_is_kept() {
        // Wide enough that one corner falls off each side in turn.
        let left_edge = on_screen_at(-500, 0, 1000, 200);
        assert!(left_edge.corners.xy[0].0 < 0);
        assert!(left_edge.corners.xy[1].0 >= 0);
        assert!(left_edge.on_screen);

        let right_edge = on_screen_at(500, 0, 1000, 200);
        assert!(right_edge.corners.xy[1].0 >= SCREEN_X_LIMIT);
        assert!(right_edge.corners.xy[0].0 < SCREEN_X_LIMIT);
        assert!(right_edge.on_screen);
    }

    #[test]
    fn a_box_fully_past_either_edge_is_dropped() {
        // Far left: 320 * -3000 / 1000 = -960, + 160 = -800, both corners.
        let far_left = on_screen_at(-3000, 0, 1000, 10);
        assert!(far_left.corners.xy[0].0 < 0 && far_left.corners.xy[1].0 < 0);
        assert!(!far_left.on_screen);

        let far_right = on_screen_at(3000, 0, 1000, 10);
        assert!(
            far_right.corners.xy[0].0 >= SCREEN_X_LIMIT
                && far_right.corners.xy[1].0 >= SCREEN_X_LIMIT
        );
        assert!(!far_right.on_screen);
    }

    #[test]
    fn no_vertical_test_at_all() {
        // Centred horizontally, driven far off the top and then far off the
        // bottom: retail reads no Y, so both are accepted. This is the
        // retail quirk, not an approximation - do not "fix" it into a
        // rectangle test.
        for y in [-30_000i16, 30_000] {
            let r = on_screen_at(0, y, 1000, 10);
            assert!(r.on_screen, "y = {y} must still read on-screen");
        }
    }

    #[test]
    fn half_size_is_both_axes() {
        // The projector gets `half_size` twice, so the box is square in view
        // space and the vertical extent scales with the same argument.
        let small = on_screen_at(0, 0, 1000, 10);
        let large = on_screen_at(0, 0, 1000, 100);
        let width = |r: &BattleActorOnScreen| r.corners.xy[1].0 - r.corners.xy[0].0;
        let height = |r: &BattleActorOnScreen| r.corners.xy[2].1 - r.corners.xy[0].1;
        assert_eq!(width(&small), height(&small));
        assert_eq!(width(&large), height(&large));
        assert!(width(&large) > width(&small));
    }

    #[test]
    fn position_is_the_seat_actors_verbatim() {
        // The copy is `lwl`/`lwr` pairs over eight bytes, so it is the seat
        // actor's SVECTOR unchanged - no clamp, no offset.
        let r = on_screen_at(-1234, 5678, 900, 4);
        assert_eq!(r.position, (-1234, 5678, 900));
        assert_eq!(r.corners.view_z, 900);
    }

    #[test]
    fn a_behind_camera_actor_still_answers() {
        // Retail has no Z gate either: SZ3 clamps to 0, the divide saturates,
        // and the same two X comparisons run on whatever the GTE latched.
        let r = on_screen_at(0, 0, -500, 10);
        assert!(r.corners.behind);
        assert_eq!(r.corners.depth, 0);
        assert!(r.on_screen);
    }
}
