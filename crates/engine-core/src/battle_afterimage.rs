//! Battle **arts after-image ghosts** - the mesh trail a Super / Miracle Art
//! dash leaves behind the character.
//!
//! PORT: FUN_80049348 (the per-actor arts after-image walk)
//!
//! Retail redraws the actor's own mesh at poses from a few frames ago. The
//! SCUS anim tick `FUN_80047430` keeps a 32-deep per-actor history ring -
//! position (`actor[+0x4C]`, 8-byte stride), anim cursor (`+0x17A`),
//! committed clip record (`+0x234`) and committed anim id (`+0x1FB`), all
//! shifted down one slot per frame with slot 0 taking the live values
//! (`0x80047E58..0x80047F0C`). The after-image walk `FUN_80049348` (run from
//! the per-actor battle draw tick `FUN_800480D8`) then draws **two** ghosts
//! from that ring:
//!
//! * **Spacing**: `step = 8 / actor[+0x21D]` (floored at 1; doubled for a
//!   monster seat), ghosts at ring depths `step` and `2 * step`
//!   (`0x800493F0..0x8004943C`) - so the trail stretches exactly when the
//!   arts slow-motion drops the rate.
//! * **Gate**: a ghost draws only when the ring's anim id at that depth is
//!   `> 0x10` (`0x80049458..0x80049464`). For a party member the committed
//!   art-clip slot is `0x11` precisely when the staged id was `0x10` or
//!   `0x1A` (`anim_vm::resolve_staged_anim`) - i.e. the Super / Miracle
//!   **SpecialStarter** dash; an ordinary art materialises at slot `0x10`
//!   and leaves the 2D weapon-trail streak instead. For a monster the ring
//!   id is `clip_tag + 0x10` (`0x80048044..0x80048060`), so any non-idle
//!   clip (tag `>= 1`) ghosts.
//! * **Colour**: the ghost is drawn **flat-coloured and additive** - the
//!   draw wrapper `FUN_80043390` decodes the colour word's mode byte
//!   (`0x85`): bit `0x80` sets the GP0 ABE (semi-transparent) command bit,
//!   low bits `0x01` = ABR mode 1 (additive), bit `0x04` selects the
//!   flat-colour prim bank with the GTE far colour = the ghost RGB. The base
//!   RGB is per-character (SCUS table `0x80076908`, indexed by character id;
//!   monsters share `0x80076914`), and each drawn ghost then steps the word
//!   down by `0x101010` (`0x80049520..0x80049530`) so the older ghost is
//!   dimmer. The ghost's OT depth is pushed `+0x50` buckets deeper than the
//!   live body (`FUN_80048A08`, the `+0x10` bit-`0x800000` arms), so it
//!   draws behind it.
//!
//! This module is the renderer-free kernel: the schedule, gate and colour
//! law. `World::battle_ghost_draws` binds it to the live pose history; each
//! host draws the returned poses as flat-coloured additive copies of the
//! actor's mesh.

/// Depth of the retail history ring (`0x1F` shifts + slot 0).
pub const HISTORY_DEPTH: usize = 32;

/// Number of ghosts one walk draws (loop `i = step; i < 2*step + 1;
/// i += step`).
pub const GHOST_COUNT: usize = 2;

/// Per-character ghost base colours `[r, g, b]` - the SCUS word table at
/// `0x80076908`, indexed `character_id - 1` (1 = Vahn, 2 = Noa, 3 = Gala;
/// `FUN_80049348` `0x8004939C..0x800493C4`). Byte order per the render
/// node's colour word: R = byte 0.
pub const GHOST_COLOR_PARTY: [[u8; 3]; 3] = [
    [0x60, 0x30, 0x30], // Vahn - red
    [0x30, 0x60, 0x30], // Noa - green
    [0x30, 0x30, 0x60], // Gala - blue
];

/// Monster ghost base colour (`DAT_80076914`, `0x800493C8..0x800493D4`).
pub const GHOST_COLOR_MONSTER: [u8; 3] = [0x50, 0x50, 0x30];

/// Per-drawn-ghost colour decay (`colour - 0x101010`, `0x80049520`).
pub const GHOST_COLOR_DECAY: u8 = 0x10;

/// Ring-id threshold: a ghost draws only for history ids **greater than**
/// `0x10` (`sltiu 0x11` at `0x80049460`).
pub const GHOST_RING_ID_MIN: u8 = 0x11;

/// The two ring depths the walk samples for an actor: `step` and `2 * step`
/// with `step = 8 / rate` (floored at 1 - retail floors the quotient's zero
/// case at `0x80049410`), doubled for a monster seat (`0x8004941C..
/// 0x80049428`). A frozen actor (`rate 0`) is clamped to the deepest pair
/// the ring can serve rather than reproducing retail's undefined
/// divide-by-zero.
pub fn ghost_depths(rate: u8, monster: bool) -> [usize; GHOST_COUNT] {
    let mut step = if rate == 0 {
        HISTORY_DEPTH / 2 - 1
    } else {
        (8 / rate as usize).clamp(1, HISTORY_DEPTH / 2 - 1)
    };
    if monster {
        step = (step * 2).min(HISTORY_DEPTH / 2 - 1);
    }
    [step, 2 * step]
}

/// One planned ghost draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostPlan {
    /// History-ring depth in frames (1 = last frame).
    pub depth: usize,
    /// Flat additive RGB for this ghost.
    pub color: [u8; 3],
}

/// Plan the walk for one actor: sample the two scheduled depths, keep the
/// ones whose history entry is ghost-eligible, and apply the per-drawn-ghost
/// colour decay (the decay steps only when a ghost is actually drawn -
/// retail decrements the live colour word inside the draw arm).
pub fn plan_ghosts(
    rate: u8,
    monster: bool,
    base: [u8; 3],
    mut eligible: impl FnMut(usize) -> bool,
) -> Vec<GhostPlan> {
    let mut out = Vec::with_capacity(GHOST_COUNT);
    let mut color = base;
    for depth in ghost_depths(rate, monster) {
        if !eligible(depth) {
            continue;
        }
        out.push(GhostPlan { depth, color });
        for c in color.iter_mut() {
            *c = c.saturating_sub(GHOST_COLOR_DECAY);
        }
    }
    out
}

/// Centre of projection (the camera eye) of a perspective view-projection
/// matrix, in the matrix's own input space. `vp` is column-major
/// (`vp[col * 4 + row]`). Returns `None` for a singular / orthographic
/// matrix (no finite eye).
///
/// The eye is the unique input point the projection collapses: rows 0, 1
/// and 3 of `vp` all evaluate to zero on `(eye, 1)` (screen centre and
/// `w = 0`), which is a 3x3 linear solve.
pub fn camera_eye_from_vp(vp: &[f32; 16]) -> Option<[f32; 3]> {
    let at = |r: usize, c: usize| vp[c * 4 + r];
    // Rows 0, 1, 3 as [a b c | d] with a*x + b*y + c*z + d = 0.
    let rows = [0usize, 1, 3].map(|r| [at(r, 0), at(r, 1), at(r, 2), at(r, 3)]);
    let m = &rows;
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    // Cramer's rule on A * eye = -d.
    let rhs = [-m[0][3], -m[1][3], -m[2][3]];
    let col = |k: usize| {
        let mut a = [[0.0f32; 3]; 3];
        for (i, row) in m.iter().enumerate() {
            for j in 0..3 {
                a[i][j] = if j == k { rhs[i] } else { row[j] };
            }
        }
        (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
            - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
            / det
    };
    Some([col(0), col(1), col(2)])
}

/// The uniform scale-about-the-eye factor that parks a ghost **behind** the
/// live body along its own camera rays (`>= 1`; `1.0` = no push needed).
///
/// Retail draws each ghost `0x50` OT buckets deeper than the body
/// (`FUN_80048A08`), which under the console's painter ordering means the
/// body covers the coincident screen area **regardless of true depth** -
/// including the attack camera's behind-the-attacker framings, where the
/// trailing ghost is genuinely *nearer* the camera than the body. A
/// depth-buffer host cannot reproduce that with any compare function or
/// fixed offset; scaling the ghost uniformly about the eye keeps its screen
/// silhouette exactly (every vertex slides along its own ray) while placing
/// it `margin` world units beyond the body's distance, so the body's depth
/// always wins where they overlap and only the separated trail shows.
pub fn ghost_eye_push_scale(
    eye: [f32; 3],
    ghost_pos: [f32; 3],
    body_pos: [f32; 3],
    margin: f32,
) -> f32 {
    let dist = |p: [f32; 3]| {
        let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    };
    let d_ghost = dist(ghost_pos).max(1.0);
    let d_body = dist(body_pos);
    ((d_body + margin) / d_ghost).max(1.0)
}

/// The default [`ghost_eye_push_scale`] margin, in raw actor-space world
/// units: comfortably past a battle mesh's camera-facing extent (a few
/// hundred units) so the ghost's own front surface clears the body's.
pub const GHOST_EYE_PUSH_MARGIN: f32 = 400.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic perspective vp (translate-then-project) hands its eye
    /// back through the row solve.
    #[test]
    fn camera_eye_recovers_from_a_synthetic_perspective() {
        // View: translate the eye (10, -20, 30) to the origin, look down +z.
        // Projection: x' = x, y' = y, z' = z, w' = z (a bare pinhole).
        let (ex, ey, ez) = (10.0f32, -20.0, 30.0);
        // Column-major: col*4 + row.
        let mut vp = [0.0f32; 16];
        vp[0] = 1.0; // x row
        vp[5] = 1.0; // y row
        vp[10] = 1.0; // z row
        vp[11] = 1.0; // w row = z_view
        // Translation column (col 3): view = world - eye.
        vp[12] = -ex;
        vp[13] = -ey;
        vp[14] = -ez;
        vp[15] = -ez; // w row's constant: w = z - ez
        let eye = camera_eye_from_vp(&vp).expect("perspective has an eye");
        assert!((eye[0] - ex).abs() < 1e-3, "{eye:?}");
        assert!((eye[1] - ey).abs() < 1e-3, "{eye:?}");
        assert!((eye[2] - ez).abs() < 1e-3, "{eye:?}");
    }

    /// An orthographic matrix (constant w row) has no eye.
    #[test]
    fn camera_eye_rejects_an_orthographic_matrix() {
        let mut vp = [0.0f32; 16];
        vp[0] = 1.0;
        vp[5] = 1.0;
        vp[10] = 1.0;
        vp[15] = 1.0; // w = 1 always
        assert_eq!(camera_eye_from_vp(&vp), None);
    }

    /// A ghost nearer the camera than the body gets pushed past it; a ghost
    /// already beyond the body + margin is left alone.
    #[test]
    fn eye_push_parks_a_near_ghost_behind_the_body() {
        let eye = [0.0, 0.0, -1000.0];
        let body = [0.0, 0.0, 600.0]; // 1600 from the eye
        let near_ghost = [0.0, 0.0, 200.0]; // 1200 from the eye
        let k = ghost_eye_push_scale(eye, near_ghost, body, 400.0);
        assert!((k - 2000.0 / 1200.0).abs() < 1e-4, "k={k}");
        // Pushed distance = body distance + margin.
        assert!((k * 1200.0 - 2000.0).abs() < 1e-2);
        let far_ghost = [0.0, 0.0, 2000.0]; // 3000 from the eye, past margin
        assert_eq!(ghost_eye_push_scale(eye, far_ghost, body, 400.0), 1.0);
    }

    #[test]
    fn normal_rate_ghosts_trail_one_and_two_frames() {
        assert_eq!(ghost_depths(8, false), [1, 2]);
        // Monster seats double the spacing.
        assert_eq!(ghost_depths(8, true), [2, 4]);
    }

    #[test]
    fn slow_motion_stretches_the_trail() {
        assert_eq!(ghost_depths(4, false), [2, 4]);
        assert_eq!(ghost_depths(2, false), [4, 8]);
        // The starter's quarter-speed actor: depths 4 and 8 - the deep
        // dash trail.
        assert_eq!(ghost_depths(2, true), [8, 16]);
    }

    #[test]
    fn frozen_rate_clamps_inside_the_ring() {
        let [a, b] = ghost_depths(0, false);
        assert!(b > a && b < HISTORY_DEPTH);
        let [a, b] = ghost_depths(0, true);
        assert!(b > a && b < HISTORY_DEPTH);
    }

    #[test]
    fn plan_keeps_only_eligible_depths_and_decays_per_drawn_ghost() {
        // Both eligible: second ghost is one decay step dimmer.
        let plans = plan_ghosts(8, false, [0x60, 0x30, 0x30], |_| true);
        assert_eq!(
            plans,
            vec![
                GhostPlan {
                    depth: 1,
                    color: [0x60, 0x30, 0x30]
                },
                GhostPlan {
                    depth: 2,
                    color: [0x50, 0x20, 0x20]
                },
            ]
        );
        // First depth ineligible: the drawn ghost still gets the base
        // colour (the decay follows draws, not depths).
        let plans = plan_ghosts(8, false, [0x60, 0x30, 0x30], |d| d == 2);
        assert_eq!(
            plans,
            vec![GhostPlan {
                depth: 2,
                color: [0x60, 0x30, 0x30]
            }]
        );
        // None eligible: no ghosts.
        assert!(plan_ghosts(8, false, GHOST_COLOR_MONSTER, |_| false).is_empty());
    }
}
