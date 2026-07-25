//! Battle **effect-ribbon** geometry: the random-walk quad chain the actor
//! render-mode-4 multi-target emitter builds (lightning bolts, beams, whips).
//!
//! PORT: FUN_801CFA48
//!
//! `FUN_8001ADA4` case 4 (multi-target) picks one of three primitive emitters
//! off the actor's `+0x9E` flags; the `0x2000` arm is this one, resident in the
//! battle overlay. All three share the call shape
//! `(out_buf, mode, packed, src)`: they zero the header's first three words,
//! build a chain at `out_buf + 0xC`, and take a primitive **count** from
//! `packed >> 8`. What is specific to this arm is the geometry - it is not a
//! static quad strip but a **random walk**:
//!
//! - the chain advances one step per primitive, each step turning by a wander
//!   angle and moving a randomised distance along it;
//! - each step emits **six** vertices at 8-byte stride (three lateral pairs at
//!   ±1, ±2 and ±8 times a randomised radius), so a step is two quads wide at
//!   the near end and flares at the far end;
//! - the radius **tapers** over the second half of the chain, so the ribbon
//!   thins toward its tip;
//! - a leading run of steps can be forced degenerate (zero radius), which is
//!   how a growing bolt is animated: the same buffer is rebuilt each frame with
//!   fewer suppressed steps.
//!
//! The emitter is the geometry half only. `801CFA48` also assembles the GPU
//! packet chain that draws the vertices (`0x3C` Gouraud-textured quads, 9 words
//! each, colour words derived from `src[+4]` / `src[+8]`); that half is
//! render-track and is described by [`RibbonPackets`] rather than emitted here.
//!
//! `see ghidra/scripts/funcs/overlay_battle_action_801cfa48.txt`. The sibling
//! dump `overlay_menu_801cfa48.txt` is only a citation pointer (its own header
//! says so) - the enclosing function there is a different one.
//!
//! REF: FUN_8001ADA4 (the render dispatcher arm that selects this emitter),
//! FUN_80028158, FUN_8002A5A4 (the other two arms), FUN_801D0290 (the RNG)

/// PSX angle units in a full revolution - the sin / cos LUT index space.
pub const ANGLE_MASK: i32 = 0xFFF;

/// Fixed-point shift of the sin / cos LUT entries.
const TRIG_SHIFT: u32 = 13;

/// Fixed-point shift of the per-step advance (the walk integrates position at
/// a coarser scale than it computes the lateral offsets).
const STEP_SHIFT: u32 = 12;

/// The base heading the walk starts from (`0x801cfbe8`: `li t8,-0x400`).
pub const RIBBON_START_ANGLE: i32 = -0x400;

/// Vertices a single ribbon step emits.
pub const RIBBON_VERTS_PER_STEP: usize = 6;

/// Byte stride between two ribbon vertices in the output buffer.
pub const RIBBON_VERT_STRIDE: usize = 8;

/// Which component of the 8-byte vertex each axis lands in.
///
/// The emitter picks one of three permutations off `mode & 3`, so the same walk
/// can be laid out in the XY, XZ or YZ plane of the consumer's vertex format.
/// Retail leaves the pointers uninitialised for `mode == 3`; the port treats
/// that as [`RibbonPlane::Xy`] rather than reading garbage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonPlane {
    /// `mode & 3 == 0`: walk-x at `+0`, walk-y at `+2`, zero at `+4`.
    Xy,
    /// `mode & 3 == 1`: walk-x at `+0`, walk-y at `+4`, zero at `+2`.
    Xz,
    /// `mode & 3 == 2`: walk-x at `+4`, walk-y at `+0`, zero at `+2`.
    Yz,
}

impl RibbonPlane {
    /// The plane `mode & 3` selects.
    pub fn from_mode(mode: u32) -> Self {
        match mode & 3 {
            1 => Self::Xz,
            2 => Self::Yz,
            _ => Self::Xy,
        }
    }

    /// Byte offsets of `(walk_x, walk_y, zero)` within the 8-byte vertex.
    pub fn component_offsets(self) -> (usize, usize, usize) {
        match self {
            Self::Xy => (0, 2, 4),
            Self::Xz => (0, 4, 2),
            Self::Yz => (4, 0, 2),
        }
    }
}

/// The ribbon's shape parameters, read out of the emitter's `src` struct.
///
/// Field names are by role; the offsets are the `src` reads at
/// `0x801cfaac..0x801cfc44`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RibbonParams {
    /// `src[+0x0C]` (`u16`) - spread of the per-step heading wander. Each step
    /// adds `rand % spread - spread / 2` to the wander accumulator.
    pub wander_spread: u16,
    /// `src[+0x18]` (`i16`) - base lateral radius; the emitter halves it.
    pub radius: i16,
    /// `src[+0x1A]` (`i16`) - base step length; the emitter halves it and adds
    /// `rand % half` per step.
    pub step_len: i16,
    /// `src[+0x1C]` (`i16`) - a per-call scalar the emitter quarters and
    /// publishes to `_DAT_801F6950` for the packet half to read.
    pub depth_cue: i16,
    /// `src[+0x1E]` (`i16`) - constant heading advance per step, on top of the
    /// wander.
    pub turn_rate: i16,
}

/// One emitted ribbon step: six vertices in `(walk_x, walk_y)` pairs, in the
/// order the emitter stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonStep {
    /// The six `(x, y)` pairs at `+0x00`, `+0x08`, `+0x10`, `+0x18`, `+0x20`
    /// and `+0x28` of the step's `0x30`-byte block. The third component is
    /// always zero.
    pub verts: [(i16, i16); RIBBON_VERTS_PER_STEP],
}

/// The header words the emitter writes at `out_buf + 0xC`, and the packet-chain
/// geometry it derives from them.
///
/// This is the render-track half, kept as data so the layout is checkable
/// without a GPU: `vertex_base` / `packet_base` are byte offsets from
/// `out_buf`, not pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonPackets {
    /// `header[+0x00]` - byte offset of the vertex block (`out_buf + 0x28`).
    pub vertex_base: usize,
    /// `header[+0x04]` - `(steps + 2) * 6`, the allocated vertex count (two
    /// steps of slack past the emitted chain).
    pub allocated_verts: usize,
    /// `header[+0x14]` - `steps * 6`, the emitted vertex count. Also the `u16`
    /// the packet header's first word carries.
    pub emitted_verts: usize,
    /// `header[+0x10]` - byte offset of the packet block
    /// (`vertex_base + allocated_verts * 8`).
    pub packet_base: usize,
    /// Bytes of packet header before the per-quad words
    /// (`0x801d0000..0x801d0034`).
    pub packet_header_bytes: usize,
    /// Bytes per emitted quad (`0x801d00c8`: `addiu t3,t3,0x24`).
    pub packet_stride: usize,
    /// Quads emitted per step (six `0x24`-byte packets per iteration of the
    /// packet loop).
    pub packets_per_step: usize,
    /// The GPU command base the colour words carry (`0x3C000000` - a Gouraud
    /// textured quad).
    pub command_base: u32,
}

/// GPU command base the emitter ORs into every colour word.
pub const RIBBON_COMMAND_BASE: u32 = 0x3C00_0000;

/// The whole emitter result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ribbon {
    /// Steps actually walked. `steps.len()` is `emitted + 1` - the emitter's
    /// loop bound is `count + 1`, so it writes one step past the count the
    /// packet half draws.
    pub steps: Vec<RibbonStep>,
    /// Vertex-component permutation.
    pub plane: RibbonPlane,
    /// Header + packet-chain layout.
    pub packets: RibbonPackets,
    /// The value the emitter publishes to `_DAT_801F6950`
    /// (`depth_cue >> 2`).
    pub depth_cue: i32,
}

/// Split the emitter's packed count argument.
///
/// PORT: FUN_801CFA48 (`0x801cfa5c..0x801cfafc`).
///
/// `packed & 0xFF` is the per-batch cap and `packed >> 8` the total. When the
/// total is non-zero the emitter clamps the batch to it and keeps the surplus
/// as a **suppression run**: the first `remainder` steps are emitted with zero
/// radius. A zero total leaves the cap untouched and the remainder at zero,
/// which is the "draw the whole ribbon" form.
pub fn split_packed_count(packed: u32) -> (i32, i32) {
    let cap = (packed & 0xFF) as i32;
    let total = (packed >> 8) as i32;
    if total == 0 {
        return (cap, 0);
    }
    if cap < total {
        (cap, total - cap)
    } else {
        (total, 0)
    }
}

/// Sin / cos source the walk integrates against.
///
/// Retail reads two `i16` LUTs through the pointers `_DAT_8007B7F8` and
/// `_DAT_8007B81C`, both indexed by a 12-bit angle. The emitter always feeds
/// the **first** table into the walk's X component and the **second** into its
/// Y component (`0x801cfd0c`/`0x801cfd28` for the lateral offsets,
/// `0x801cff8c`/`0x801cffa8` for the advance), so that pairing - not the
/// sin-versus-cos naming - is what the port depends on. `sin` here is the
/// `_DAT_8007B7F8` table and `cos` the `_DAT_8007B81C` one, following the
/// naming the subsystem docs already use.
pub trait TrigTable {
    /// The `_DAT_8007B7F8` table (walk X) in `1 << 13` fixed point, angle
    /// masked to 12 bits.
    fn sin(&self, angle: i32) -> i32;
    /// The `_DAT_8007B81C` table (walk Y) in `1 << 13` fixed point, angle
    /// masked to 12 bits.
    fn cos(&self, angle: i32) -> i32;
}

/// A `4096`-entry analytic table in the retail LUTs' fixed point. Not a lift of
/// the disc tables - it is generated, and exists so the walk is testable
/// without disc data.
#[derive(Debug, Clone)]
pub struct AnalyticTrig {
    sin: Vec<i16>,
}

impl Default for AnalyticTrig {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticTrig {
    /// Build the table.
    pub fn new() -> Self {
        let one = f64::from(1 << TRIG_SHIFT);
        let sin = (0..4096)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / 4096.0;
                (a.sin() * one).round() as i16
            })
            .collect();
        Self { sin }
    }
}

impl TrigTable for AnalyticTrig {
    fn sin(&self, angle: i32) -> i32 {
        i32::from(self.sin[(angle & ANGLE_MASK) as usize])
    }
    fn cos(&self, angle: i32) -> i32 {
        i32::from(self.sin[((angle + 1024) & ANGLE_MASK) as usize])
    }
}

/// Narrowing the emitter uses on the **lateral** trig products:
/// `(v + (v >>> 31)) >> 13` - add the sign bit back before the arithmetic shift
/// so the result truncates toward zero.
fn narrow_lateral(v: i64) -> i32 {
    let v = v as i32;
    (v.wrapping_add(((v as u32) >> 31) as i32)) >> TRIG_SHIFT
}

/// Narrowing the emitter uses on the **advance** trig products: a plain
/// arithmetic `>> 12`, with no sign-bit correction, so a negative advance
/// floors instead of truncating. The asymmetry with [`narrow_lateral`] is in
/// the bytes (`0x801cfd58` adds the sign bit, `0x801cffd0` does not) and it
/// biases a leftward walk by one unit per step.
fn narrow_advance(v: i64) -> i32 {
    (v as i32) >> STEP_SHIFT
}

/// Build a ribbon.
///
/// PORT: FUN_801CFA48 (`0x801cfa48..0x801d028c`).
///
/// `mode` is the emitter's second argument (only its low two bits are read),
/// `packed` the third, `params` the fields it reads out of the fourth, and
/// `rand` a source of the RNG values `FUN_801D0290` supplies - the emitter
/// calls it **four** times per step, in this order:
///
/// 1. the near lateral radius `radius/2 + r % radius`,
/// 2. the mid lateral radius `radius + r % radius`,
/// 3. the wander roll: `r & 7 == 0` damps the accumulator, otherwise a fifth
///    call adds `r % spread - spread/2`,
/// 4. the step length `step/2 + r % (step/2)`.
///
/// The damping arm is asymmetric in the bytes and the port keeps it that way:
/// a negative accumulator is folded twice (magnitude `/16`, sign preserved),
/// a positive one once (magnitude `/4`, sign flipped). See [`damp_wander`].
///
/// The RNG modulus is guarded at `1`; retail divides by the raw radius / step
/// and would trap on a zero one, which the emitter is never handed.
///
/// NOT WIRED: nothing in this crate emits actor render-mode-4 primitives.
/// The emitter's only retail caller is the render dispatcher `FUN_8001ADA4`
/// case 4, which is render-track - `engine-core` is renderer-free and carries
/// no GPU packet chain, no `_DAT_801F6950` depth-cue channel, and no
/// `actor[+0x9E]` flag word to select the arm from. Wiring this needs the
/// battle effect renderer to ask for ribbon geometry per frame; the emitter is
/// pure and takes its RNG and LUTs as parameters precisely so that consumer can
/// be `engine-render` rather than this crate.
pub fn build_ribbon<T: TrigTable, R: FnMut() -> u32>(
    mode: u32,
    packed: u32,
    params: RibbonParams,
    trig: &T,
    mut rand: R,
) -> Ribbon {
    let (count, suppressed) = split_packed_count(packed);
    let plane = RibbonPlane::from_mode(mode);
    let half = (count >> 1) + 1;
    let base_radius = i32::from(params.radius) >> 1;
    let base_step = i32::from(params.step_len) >> 1;
    let spread = i32::from(params.wander_spread);

    let mut angle = RIBBON_START_ANGLE;
    let mut wander = 0i32;
    let mut walk_x = 0i32;
    let mut walk_y = 0i32;
    let mut steps: Vec<RibbonStep> = Vec::new();

    // Retail's loop bound is `i < count + 1`, so a `count` of 0 still writes
    // one step and a negative count writes none.
    for i in 0..(count + 1).max(0) {
        // Radius for this step: 1 at the head, tapering over the second half.
        let radius = if half < i {
            let scaled = base_radius * (half - (i - half));
            let r = if half != 0 { scaled / half } else { scaled };
            if r > 0 { r } else { 1 }
        } else if i == 0 {
            1
        } else {
            base_radius
        };
        let suppress = i < suppressed;
        let modulus = radius.max(1);

        // Pair 1 + 2: near lateral offset at +-1r and +-2r.
        let r1 = (radius >> 1) + (rand() % modulus as u32) as i32;
        let (dx1, dy1) = if suppress {
            (0, 0)
        } else {
            (
                narrow_lateral(i64::from(trig.sin(angle)) * i64::from(r1)),
                narrow_lateral(i64::from(trig.cos(angle)) * i64::from(r1)),
            )
        };
        // Pair 3: far lateral offset at +-8r.
        let r2 = radius + (rand() % modulus as u32) as i32;
        let (dx2, dy2) = if suppress {
            (0, 0)
        } else {
            (
                narrow_lateral(i64::from(trig.sin(angle)) * i64::from(r2)),
                narrow_lateral(i64::from(trig.cos(angle)) * i64::from(r2)),
            )
        };
        let w = |v: i32| v as i16;
        steps.push(RibbonStep {
            verts: [
                (w(walk_x - dx1), w(walk_y - dy1)),
                (w(walk_x + dx1), w(walk_y + dy1)),
                (w(walk_x - dx1 * 2), w(walk_y - dy1 * 2)),
                (w(walk_x + dx1 * 2), w(walk_y + dy1 * 2)),
                (w(walk_x - dx2 * 8), w(walk_y - dy2 * 8)),
                (w(walk_x + dx2 * 8), w(walk_y + dy2 * 8)),
            ],
        });

        // Wander roll.
        if rand() & 7 == 0 {
            wander = damp_wander(wander);
        } else if spread > 0 {
            wander += (rand() % spread as u32) as i32 - (spread >> 1);
        } else {
            // A zero spread would divide by zero in retail; the emitter is
            // only ever handed a non-zero one. Consume the roll so the RNG
            // stream stays aligned.
            let _ = rand();
        }
        angle += i32::from(params.turn_rate);

        // Advance the walk.
        let step_mod = base_step.max(1);
        let len = base_step + (rand() % step_mod as u32) as i32;
        let heading = (angle + wander) & ANGLE_MASK;
        walk_x += narrow_advance(i64::from(trig.sin(heading)) * i64::from(len));
        walk_y += narrow_advance(i64::from(trig.cos(heading)) * i64::from(len));
    }

    let emitted = (count.max(0) as usize) * RIBBON_VERTS_PER_STEP;
    let allocated = (count.max(0) as usize + 2) * RIBBON_VERTS_PER_STEP;
    Ribbon {
        steps,
        plane,
        packets: RibbonPackets {
            vertex_base: 0x28,
            allocated_verts: allocated,
            emitted_verts: emitted,
            packet_base: 0x28 + allocated * RIBBON_VERT_STRIDE,
            packet_header_bytes: 8,
            packet_stride: 0x24,
            packets_per_step: 6,
            command_base: RIBBON_COMMAND_BASE,
        },
        depth_cue: i32::from(params.depth_cue) >> 2,
    }
}

/// The wander accumulator's damping fold - the arm taken when the third RNG
/// roll of a step comes up `r & 7 == 0`.
///
/// PORT: FUN_801CFA48 (`0x801cfeec..0x801cff44`).
///
/// The bytes are asymmetric and the asymmetry is real, not a decompiler
/// artifact: the negative arm falls **through** into the positive arm, so it
/// applies both folds.
///
/// - `w < 0`: `w = -(w >> 2)` makes it positive, then the positive arm runs on
///   the result, so the net effect is magnitude `/16` with the sign preserved.
/// - `w > 0`: `w = -(w >> 2)` - magnitude `/4`, sign flipped.
/// - `w == 0`: unchanged.
///
/// Both shifts are arithmetic (floor, not truncate), so both arms collapse
/// small magnitudes to exactly `0`: `1..=3` from above and `-12..=-1` from
/// below. The negative arm's branch that would skip the second fold
/// (`bgez v0` at `0x801cfef4`) can never be taken - `w >> 2` of a negative `w`
/// is at most `-1` - so the fall-through really is unconditional.
pub fn damp_wander(w: i32) -> i32 {
    let mut w = w;
    if w < 0 {
        w = -(w >> 2);
    }
    if w > 0 {
        w = -(w >> 2);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> RibbonParams {
        RibbonParams {
            wander_spread: 0x40,
            radius: 0x80,
            step_len: 0x200,
            depth_cue: 0x40,
            turn_rate: 0x20,
        }
    }

    /// Deterministic stand-in for `FUN_801D0290`.
    fn lcg() -> impl FnMut() -> u32 {
        let mut s: u32 = 0x1234_5678;
        move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s >> 8
        }
    }

    #[test]
    fn packed_count_splits_into_batch_and_suppression_run() {
        // total 0: the cap passes through, nothing suppressed.
        assert_eq!(split_packed_count(0x0000_000A), (10, 0));
        // cap >= total: the batch is the total.
        assert_eq!(split_packed_count(0x0000_050A), (5, 0));
        // cap < total: surplus becomes the suppression run.
        assert_eq!(split_packed_count(0x0000_0A05), (5, 5));
    }

    #[test]
    fn plane_permutations_are_distinct_and_cover_three_components() {
        for mode in 0..4u32 {
            let (a, b, c) = RibbonPlane::from_mode(mode).component_offsets();
            let mut v = [a, b, c];
            v.sort_unstable();
            assert_eq!(v, [0, 2, 4], "mode {mode} must cover +0/+2/+4 exactly once");
        }
        assert_eq!(RibbonPlane::from_mode(0), RibbonPlane::Xy);
        assert_eq!(RibbonPlane::from_mode(1), RibbonPlane::Xz);
        assert_eq!(RibbonPlane::from_mode(2), RibbonPlane::Yz);
        // Retail leaves mode 3 uninitialised; the port pins it to the mode-0
        // layout rather than reading stale pointers.
        assert_eq!(RibbonPlane::from_mode(3), RibbonPlane::Xy);
    }

    #[test]
    fn damping_preserves_a_negative_sign_and_flips_a_positive_one() {
        // -100 -> -(-100>>2) = 25 -> -(25>>2) = -6: sign kept, /16.
        assert_eq!(damp_wander(-100), -6);
        // 100 -> -(100>>2) = -25: sign flipped, /4.
        assert_eq!(damp_wander(100), -25);
        assert_eq!(damp_wander(0), 0);
        // Both arms floor toward zero, so small magnitudes damp out entirely:
        // 1..=3 from above and -12..=-1 from below.
        for w in [1, 2, 3] {
            assert_eq!(damp_wander(w), 0, "w = {w}");
        }
        for w in -12..0 {
            assert_eq!(damp_wander(w), 0, "w = {w}");
        }
        // The first magnitude that survives on each side.
        assert_eq!(damp_wander(-13), -1);
        assert_eq!(damp_wander(4), -1);
        // Sign is preserved from below and flipped from above at every
        // surviving magnitude.
        for w in [-13, -16, -100, -4096] {
            assert!(damp_wander(w) < 0, "w = {w}");
        }
        for w in [4, 16, 100, 4096] {
            assert!(damp_wander(w) < 0, "w = {w}");
        }
        // Magnitude ratios: /16 from below, /4 from above.
        assert_eq!(damp_wander(-4096), -256);
        assert_eq!(damp_wander(4096), -1024);
    }

    #[test]
    fn header_layout_allocates_two_steps_of_slack() {
        let r = build_ribbon(0, 0x0000_0008, params(), &AnalyticTrig::new(), lcg());
        assert_eq!(r.packets.emitted_verts, 8 * RIBBON_VERTS_PER_STEP);
        assert_eq!(r.packets.allocated_verts, 10 * RIBBON_VERTS_PER_STEP);
        assert_eq!(
            r.packets.packet_base,
            r.packets.vertex_base + r.packets.allocated_verts * RIBBON_VERT_STRIDE
        );
        // The emitter walks one step past the drawn count.
        assert_eq!(r.steps.len(), 9);
        assert_eq!(r.depth_cue, 0x40 >> 2);
    }

    #[test]
    fn a_suppression_run_leaves_the_leading_steps_degenerate() {
        // cap 3, total 8 -> batch 3, suppression run 5. Retail then walks
        // `batch + 1` steps, so every emitted step is inside the run.
        let r = build_ribbon(0, 0x0000_0803, params(), &AnalyticTrig::new(), lcg());
        for (i, s) in r.steps.iter().enumerate() {
            let head = s.verts[0];
            let all_same = s.verts.iter().all(|&v| v == head);
            assert!(all_same, "step {i} should be degenerate: {:?}", s.verts);
        }
    }

    #[test]
    fn an_unsuppressed_ribbon_flares_and_the_head_is_thin() {
        let r = build_ribbon(0, 0x0000_0010, params(), &AnalyticTrig::new(), lcg());
        let spread = |s: &RibbonStep| {
            let xs: Vec<i32> = s.verts.iter().map(|v| i32::from(v.0)).collect();
            let ys: Vec<i32> = s.verts.iter().map(|v| i32::from(v.1)).collect();
            (xs.iter().max().unwrap() - xs.iter().min().unwrap())
                + (ys.iter().max().unwrap() - ys.iter().min().unwrap())
        };
        // Step 0 forces radius 1, so it is much thinner than a mid step.
        assert!(
            spread(&r.steps[0]) < spread(&r.steps[4]),
            "head {} should be thinner than mid {}",
            spread(&r.steps[0]),
            spread(&r.steps[4])
        );
        // The walk actually moves.
        let head = r.steps[0].verts[0];
        let tail = r.steps.last().unwrap().verts[0];
        assert_ne!(head, tail);
    }

    #[test]
    fn a_zero_count_still_emits_one_step() {
        let r = build_ribbon(0, 0, params(), &AnalyticTrig::new(), lcg());
        assert_eq!(r.steps.len(), 1);
        assert_eq!(r.packets.emitted_verts, 0);
    }

    #[test]
    fn the_same_rng_stream_reproduces_the_same_ribbon() {
        let t = AnalyticTrig::new();
        let a = build_ribbon(1, 0x0000_0020, params(), &t, lcg());
        let b = build_ribbon(1, 0x0000_0020, params(), &t, lcg());
        assert_eq!(a, b);
    }
}
