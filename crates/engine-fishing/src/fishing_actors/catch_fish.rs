// --- The landed fish's body (FUN_801D4948 sub-state 4, FUN_801D5298) ------

use legaia_minigame_formats::fishing_species::CatchPose;

/// Scene-model index of species `0`'s body: the hook spawns the fish actor
/// with model word `_DAT_8007B6F8 + species + 0x1C` (`0x801D3694..0x801D36AC`),
/// and the result actor takes the same word (`0x801D2144..0x801D2168`) - the
/// venue bundle's models `0x1C..=0x25`, after the three rods `0x19..=0x1B`.
pub const CATCH_FISH_MODEL_BASE: i16 = 0x1C;

/// The result actor's depth in front of the eye (`+0x18 = 0x3E8`,
/// `0x801D532C..0x801D5334`).
pub const PLATE_FISH_DEPTH: i32 = 0x3E8;

/// The result actor's first height, before the species' hang offset
/// (`0x7D0 - hang`, `0x801D52E8`).
pub const PLATE_FISH_TOP: i32 = 0x7D0;

/// The fishing line the result actor hangs its fish from: one Gouraud line
/// (`0x50` packet, `0x801D538C..0x801D53E8`) from the top of the frame,
/// colour [`PLATE_LINE_TOP_RGB`], to the projected tie vertex, colour
/// [`PLATE_LINE_TIE_RGB`].
pub const PLATE_LINE_TOP_RGB: [u8; 3] = [0x30, 0x30, 0x30];

/// See [`PLATE_LINE_TOP_RGB`].
pub const PLATE_LINE_TIE_RGB: [u8; 3] = [0x80, 0x80, 0x80];

/// Where this frame draws the landed fish.
///
/// REF: FUN_801d4948 (sub-state 4: the skip bit and the lifted point)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CatchFishPlace {
    /// The hooked-fish actor through the catch close-up: its flag `0x2`
    /// (the render dispatcher's skip bit, `& 0xA`) is cleared only in
    /// sub-state `4` (`0x801D4AE8..0x801D4B00`) and set again on the frame
    /// the lift seats the result (`0x801D520C`), so the body shows exactly
    /// through the lift. It draws as an ordinary node: under the camera
    /// matrix, no rotation of its own, at this raw world point.
    World([i32; 3]),
    /// The result actor (`FUN_801D5298`): it draws its own body each frame
    /// through `FUN_801D67BC` with `+0x52 = 0x420`, whose bit `0x400` locks
    /// the node to the camera (`FUN_8001CF50`): the base matrix in place of
    /// the view rotation, and the position `(0, y, PLATE_FISH_DEPTH)` taken
    /// as eye-space (times the base scale). `rot` is the species' pose.
    CameraLocked { y: i32, rot: [i16; 3] },
}

/// The landed fish this frame: which body, how large, where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CatchFish {
    /// The species id (`DAT_801D91CC`).
    pub species: usize,
    /// The render scale `+0x72` (`DAT_801D9108`, the hook's `strength + 0x400`).
    pub scale: u16,
    /// Where it draws.
    pub place: CatchFishPlace,
}

impl CatchFish {
    /// The body's scene-model index ([`CATCH_FISH_MODEL_BASE`] `+ species`).
    pub fn model(&self) -> i16 {
        CATCH_FISH_MODEL_BASE + self.species as i16
    }

    /// The venue ANM record the body plays: both actors take clip
    /// `species + 1` (`+0x5C`, `0x801D36C0` / `0x801D2194`), and a clip `n`
    /// is the scene bank's record `n - 1`.
    pub fn clip_record(&self) -> usize {
        self.species
    }

    /// The linear part that takes a posed model vertex into the place's
    /// frame - raw world units for [`CatchFishPlace::World`], eye-space world
    /// units (retail eye space over the base scale) for
    /// [`CatchFishPlace::CameraLocked`]. Row-major.
    pub fn linear(&self) -> [[f32; 3]; 3] {
        match self.place {
            CatchFishPlace::World(_) => {
                let s = f32::from(self.scale) / 4096.0;
                [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]]
            }
            CatchFishPlace::CameraLocked { rot, .. } => camera_locked_linear(rot, self.scale),
        }
    }

    /// Where the model's origin sits: a raw world point, or an eye-space
    /// point in world units.
    pub fn origin(&self) -> [f32; 3] {
        match self.place {
            CatchFishPlace::World(p) => p.map(|v| v as f32),
            CatchFishPlace::CameraLocked { y, .. } => [0.0, y as f32, PLATE_FISH_DEPTH as f32],
        }
    }
}

/// The result actor's height for a plate counter `ramp` (`+0x1A`): its first
/// tick seats `PLATE_FISH_TOP - hang`, and every tick adds `4 * step` to the
/// counter and takes `18 * step` off the height, held at `-hang`
/// (`0x801D52F4..0x801D5388`). The fish rises into the frame from below and
/// settles.
pub fn plate_fish_y(ramp: i32, hang: i16) -> i32 {
    let hang = i32::from(hang);
    (PLATE_FISH_TOP - hang - ramp * 9 / 2).max(-hang)
}

/// The camera-locked body's matrix, as the GTE builds it, over the base
/// scale - eye-space world units per model unit.
///
/// `FUN_801D67BC` loads the base block `0x8007BF10` (`6 * I`), turns it by
/// the actor's angles in the order `+0x26`, `+0x24`, `+0x28`
/// (`RotMatrixY`, `X`, `Z`, each post-multiplying the GTE rotation), and
/// stores `6 * Ry Rx Rz`. The lock arm of `FUN_8001CF50` then multiplies the
/// base onto it once more (`MulMatrix2` through `MVMVA` at `sf = 1`, `lm = 0`),
/// and each product element saturates at the 16-bit `IR` limit - so an
/// axis-aligned pose (every disc pose is one) comes out at eight times, not
/// thirty-six. The render scale `+0x72` scales it last (`0x801D68EC..`):
/// `ScaleMatrix` (`FUN_8005B4E8`) takes each element times the scale `>> 12`
/// and keeps the low halfword, so past `0x1000` a saturated element wraps.
/// No hook strength reaches that (the plate fish's scale stays under
/// `0x1000`); the port wraps as the routine does.
///
/// PORT: FUN_801d67bc (the result actor's model matrix)
/// REF: FUN_8001cf50 (the `0x400` camera-lock arm)
pub fn camera_locked_linear(rot: [i16; 3], scale: u16) -> [[f32; 3]; 3] {
    let n = mul3(mul3(rot_y(rot[1]), rot_x(rot[0])), rot_z(rot[2]));
    let base = 6.0f64 * 4096.0;
    n.map(|row| {
        row.map(|e| {
            // `6 * N` as the GTE holds it (q3.12), then `base * that >> 12`
            // into a saturating IR, then the scale's halfword product; back
            // to a ratio over the base scale.
            let first = (6.0 * e * 4096.0).round();
            let second = (base * first / 4096.0).clamp(-32768.0, 32767.0) as i32;
            let scaled = (second * i32::from(scale)) >> 12;
            (f64::from(scaled as i16) / base) as f32
        })
    })
}

fn angle(a: i16) -> (f64, f64) {
    (f64::from(a) * std::f64::consts::TAU / 4096.0).sin_cos()
}

fn rot_x(a: i16) -> [[f64; 3]; 3] {
    let (s, c) = angle(a);
    [[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]
}

fn rot_y(a: i16) -> [[f64; 3]; 3] {
    let (s, c) = angle(a);
    [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
}

fn rot_z(a: i16) -> [[f64; 3]; 3] {
    let (s, c) = angle(a);
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}

fn mul3(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}

/// The landed fish this frame, from the session and the venue camera
/// driver: the lifted body through the close-up, the result actor's body on
/// the plate after it, `None` otherwise. `poses` is the species' showcase
/// table ([`legaia_minigame_formats::fishing_species::parse_catch_poses`]);
/// with no pose for the species the plate shows no body.
///
/// PORT: FUN_801d5298 (the result actor's body: its height ladder and pose)
pub fn catch_fish(
    session: &crate::fishing::PondSession,
    camera: &super::VenueCameraDriver,
    poses: &[CatchPose],
) -> Option<CatchFish> {
    if session.phase() != crate::fishing::PondPhase::Landed {
        return None;
    }
    let species = session.hooked()?.index;
    let scale = session.fish_scale();
    if let Some((x, y, z)) = camera.lift_point() {
        return Some(CatchFish {
            species,
            scale,
            place: CatchFishPlace::World([i32::from(x), i32::from(y), i32::from(z)]),
        });
    }
    let result = session.catch_result()?;
    let pose = poses.get(species)?;
    Some(CatchFish {
        species,
        scale,
        place: CatchFishPlace::CameraLocked {
            y: plate_fish_y(result.ramp, pose.hang),
            rot: pose.rot,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plate_fish_rises_from_below_and_settles_at_its_hang() {
        assert_eq!(plate_fish_y(0, 0), 0x7D0);
        assert_eq!(plate_fish_y(4, 0), 0x7D0 - 18, "one step-1 tick");
        assert_eq!(plate_fish_y(0x1000, 0), 0);
        assert_eq!(plate_fish_y(0x1000, -100), 100, "a positive rest");
        assert_eq!(plate_fish_y(8, -300), 0x7D0 + 300 - 36);
    }

    #[test]
    fn an_axis_aligned_lock_saturates_to_eight_times() {
        // The identity pose: 6 * 6 = 36 saturates to the IR limit, ~8x.
        let m = camera_locked_linear([0, 0, 0], 0x1000);
        assert!((m[0][0] - 32767.0 / 4096.0 / 6.0).abs() < 1e-4);
        assert_eq!(m[0][1], 0.0);
        // Species 5's pose `(0x400, -0x400, 0)`: Ry(-90) Rx(90).
        let m = camera_locked_linear([0x400, -0x400, 0], 0x800);
        let k = 32767.0 / 4096.0 / 6.0 / 2.0;
        let neg = 32768.0 / 4096.0 / 6.0 / 2.0;
        // Ry(-90) Rx(90) = [[0,-1,0],[0,0,-1],[1,0,0]].
        assert!((m[0][1] + neg).abs() < 1e-4, "{m:?}");
        assert!((m[1][2] + neg).abs() < 1e-4, "{m:?}");
        assert!((m[2][0] - k).abs() < 1e-4, "{m:?}");
        assert!(m[0][0].abs() < 1e-4 && m[1][1].abs() < 1e-4 && m[2][2].abs() < 1e-4);
    }

    #[test]
    fn the_lift_body_is_a_scaled_world_node() {
        let f = CatchFish {
            species: 5,
            scale: 0x52B,
            place: CatchFishPlace::World([5881, -196, 5182]),
        };
        assert_eq!(f.model(), 0x21);
        assert_eq!(f.clip_record(), 5);
        assert_eq!(f.origin(), [5881.0, -196.0, 5182.0]);
        assert!((f.linear()[1][1] - 0x52B as f32 / 4096.0).abs() < 1e-6);
    }
}
