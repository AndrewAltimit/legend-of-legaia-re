//! The line-segment clippers: the 3-D near clip + projection and the 2-D window clip.

// --- 3-D segment clip + projection (FUN_801D5C2C) --------------------------

/// Screen-space centre the projector biases both outputs by (`0xA0`, `0x78`).
pub const SCREEN_CENTRE: (i16, i16) = (0xA0, 0x78);

/// A segment that survived the near-plane reject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedSegment {
    /// The view-space endpoints after clipping, as retail writes them back
    /// through the caller's two `i16[3]` buffers.
    pub view: [[i16; 3]; 2],
    /// The projected screen positions of those endpoints.
    pub screen: [(i16, i16); 2],
}

/// Fixed-point lerp helper: `(delta * t) >> 12`, biased so a negative
/// product truncates toward zero exactly as the retail `addiu ,0xfff` does.
#[inline]
fn lerp12(delta: i32, t: i32) -> i32 {
    let p = delta.wrapping_mul(t);
    let p = if p < 0 { p.wrapping_add(0xFFF) } else { p };
    p >> 12
}

/// Clip a view-space segment against the near bound and project both ends.
///
/// `a` and `b` are the endpoints **already transformed into view space** (in
/// retail, by the GTE wrapper `FUN_8003D344`, one `MVMVA`). `near` is the
/// depth bound at scratchpad `0x1F80037E` and `proj` the projection distance
/// at `0x8007B6F4`.
///
/// Returns `None` for the whole-segment reject - when *both* endpoints are
/// nearer than `near`, retail zeroes the two screen outputs and leaves the
/// endpoint buffers untouched.
///
/// The two clip arms are **not symmetric**, and the port keeps the
/// asymmetry. The `a`-side arm solves for the near crossing correctly:
/// `t = ((b.z - near) << 12) / (a.z - b.z)`, then `a = b - t * (a - b)`.
/// The `b`-side arm reuses the same numerator against the opposite
/// denominator - `t = ((b.z - near) << 12) / (b.z - a.z)`, then
/// `b = a + t * (b - a)` - which is the *complement* of the parameter that
/// would put `b` on the near plane. Only `b.z` is then forced to `near`, so
/// the far endpoint's x/y slide by `1 - t` instead of `t`.
///
/// One deliberate deviation: retail reaches the R3000 divide-by-zero trap
/// when a denominator or a post-clip `z` is zero. The port returns `None`
/// for those instead of trapping, which is the same "nothing to draw"
/// outcome the reject path produces.
///
/// PORT: FUN_801d5c2c
// REPLACED-BY: nothing is owed a port - **retail reaches this routine from
// nowhere.** A five-form reference sweep (literal LE word at every alignment,
// `lui`+`addiu` / `ori` materialisation, `jal`, `j`, PC-relative branch) over
// `SCUS_942.54`, every base-mapped overlay image and every raw PROT entry
// finds zero references to `0x801D5C2C`, and the fishing overlay holds exactly
// one literal pointer anywhere in the surrounding `0x801D5000..0x801D63FF`
// band, so it is not reached as `table_base + index` either. It is a real
// prologue entry point (`locate-entry-image.py` frames it in PROT 0972 and in
// no other image) that nothing calls - dead code the linker kept, and so a
// row the wiring worklist can never close (`docs/tooling/port-catalog.md` -
// What may carry it, fourth shape).
//
// So this is not the same row as [`clip_segment_2d`] below, which is live
// retail code with one caller. Naming a missing line primitive here would imply
// a call site could exist; none can. The port keeps the routine because the
// arithmetic is documented ground truth for the projection those overlays use,
// not because a host is owed.
pub fn project_segment(a: [i32; 3], b: [i32; 3], near: i32, proj: i32) -> Option<ProjectedSegment> {
    if a[2] < near && b[2] < near {
        return None;
    }
    let mut p = a;
    let mut q = b;

    if p[2] < near {
        let denom = a[2] - b[2];
        if denom == 0 {
            return None;
        }
        let t = ((b[2] - near) << 12) / denom;
        p[0] = b[0] - lerp12(a[0] - b[0], t);
        p[1] = b[1] - lerp12(a[1] - b[1], t);
        p[2] = near;
        q = b;
    }
    if q[2] < near {
        let denom = b[2] - a[2];
        if denom == 0 {
            return None;
        }
        let t = ((b[2] - near) << 12) / denom;
        p = a;
        q[0] = a[0] + lerp12(b[0] - a[0], t);
        q[1] = a[1] + lerp12(b[1] - a[1], t);
        q[2] = near;
    }

    let view = [
        [p[0] as i16, p[1] as i16, p[2] as i16],
        [q[0] as i16, q[1] as i16, q[2] as i16],
    ];
    let scale = proj << 12;
    let project = |v: [i16; 3]| -> (i16, i16) {
        let k = scale / (v[2] as i32);
        (
            (lerp12(v[0] as i32, k) + SCREEN_CENTRE.0 as i32) as i16,
            (lerp12(v[1] as i32, k) + SCREEN_CENTRE.1 as i32) as i16,
        )
    };
    if view[0][2] == 0 || view[1][2] == 0 {
        return None;
    }
    Some(ProjectedSegment {
        view,
        screen: [project(view[0]), project(view[1])],
    })
}

// --- 2-D segment clip (FUN_801D56E4) ---------------------------------------

/// The four scratchpad halfwords the 2-D clipper reads, in the order it
/// reads them: `x_min` (`0x1F800388`, offset `+0x74` off the render block at
/// `0x1F800314`), `y_min` (`+0x76`), `x_max` (`+0x78`), `y_max` (`+0x7A`).
///
/// Retail sign-extends each bound on the *comparison* and reloads it
/// **zero-extended** (`lhu`) for the store, so a bound with the top bit set
/// compares negative and stores positive. The port keeps the bounds signed;
/// the retail window is a screen rectangle and never reaches that case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRect {
    pub x_min: i16,
    pub y_min: i16,
    pub x_max: i16,
    pub y_max: i16,
}

// Wired through [`fishing_line`], the fishing line's per-frame build: retail's
// one caller (`jal` at `0x801D3D00`, the tail of the per-frame lure tick
// `FUN_801D26CC`) clips the line packet's two endpoints against
// [`LINE_CLIP_RECT`] before linking it. `crate::fishing::PondSession::line_frame`
// runs it every frame a line is out, on all three hosts (see
// `docs/subsystems/minigame-fishing.md` § "The fishing line").
// (`project_segment` above is a different case - retail never calls it at all.)
/// Clip a 2-D segment in place against [`ClipRect`].
///
/// `p` and `q` are the two `(x, y)` endpoints; both are edited. Retail runs
/// **eight** arms - each of the four bounds is applied to each endpoint in
/// turn, in the order `x_min(p)`, `x_min(q)`, `x_max(p)`, `x_max(q)`,
/// `y_min(p)`, `y_min(q)`, `y_max(p)`, `y_max(q)` - and each arm fires only
/// when the endpoint it moves is outside the bound **and the other endpoint
/// is strictly inside it**, so a segment wholly outside one bound is left
/// alone rather than collapsed.
///
/// Every arm has the same fixed-point form. For the `x_min` arm on `p`:
/// `t = ((q.x - bound) << 12) / (q.x - p.x)`, then
/// `p.y = q.y + (((p.y - q.y) * t) >> 12)` with the `+0xFFF` bias that
/// truncates a negative product toward zero, then `p.x = bound`. The `y`
/// arms are the same with the roles of the two components swapped.
///
/// The parameter is measured from the **other** endpoint, which is why the
/// blend is written against `q` rather than against `p`.
///
/// One deliberate deviation: retail reaches the R3000 divide-by-zero trap
/// when the two endpoints share the component being clipped. That cannot
/// happen on a firing arm - the arm requires one endpoint strictly below the
/// bound and the other strictly above it, so the difference is non-zero - but
/// the port returns without editing rather than trapping if it ever is.
///
/// PORT: FUN_801d56e4
pub fn clip_segment_2d(p: &mut (i16, i16), q: &mut (i16, i16), rect: ClipRect) {
    // `lo`: the endpoint sits below the bound and the other above it, so the
    // low side is clipped up onto it. `hi` is the mirror.
    fn arm_lo(a: &mut (i16, i16), b: (i16, i16), bound: i16, vertical: bool) {
        let (ac, bc) = if vertical { (a.1, b.1) } else { (a.0, b.0) };
        if !((ac as i32) < bound as i32 && (bound as i32) < bc as i32) {
            return;
        }
        blend(
            a,
            b,
            bound,
            vertical,
            (bc as i32 - bound as i32) << 12,
            bc,
            ac,
        );
    }
    fn arm_hi(a: &mut (i16, i16), b: (i16, i16), bound: i16, vertical: bool) {
        let (ac, bc) = if vertical { (a.1, b.1) } else { (a.0, b.0) };
        if !((bound as i32) < ac as i32 && (bc as i32) < bound as i32) {
            return;
        }
        blend(
            a,
            b,
            bound,
            vertical,
            (bound as i32 - bc as i32) << 12,
            ac,
            bc,
        );
    }
    fn blend(
        a: &mut (i16, i16),
        b: (i16, i16),
        bound: i16,
        vertical: bool,
        num: i32,
        den_hi: i16,
        den_lo: i16,
    ) {
        let den = den_hi as i32 - den_lo as i32;
        if den == 0 {
            return;
        }
        let t = num / den;
        if vertical {
            a.0 = (b.0 as i32 + lerp12(a.0 as i32 - b.0 as i32, t)) as i16;
            a.1 = bound;
        } else {
            a.1 = (b.1 as i32 + lerp12(a.1 as i32 - b.1 as i32, t)) as i16;
            a.0 = bound;
        }
    }

    arm_lo(p, *q, rect.x_min, false);
    arm_lo(q, *p, rect.x_min, false);
    arm_hi(p, *q, rect.x_max, false);
    arm_hi(q, *p, rect.x_max, false);
    arm_lo(p, *q, rect.y_min, true);
    arm_lo(q, *p, rect.y_min, true);
    arm_hi(p, *q, rect.y_max, true);
    arm_hi(q, *p, rect.y_max, true);
}
