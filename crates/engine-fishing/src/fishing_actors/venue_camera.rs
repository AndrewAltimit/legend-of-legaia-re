//! The fishing venue's scripted camera: the retail camera globals and every
//! write the fishing overlay makes to them, one method per write site.
//!
//! Retail has no free camera in the fishing mode. Nine globals hold the view -
//! the rotation trio `_DAT_8007B790` (pitch) / `_DAT_8007B792` (yaw) /
//! `_DAT_8007B794` (roll), the eye translation `TR` at `_DAT_800840B8..C0`
//! and the focus translation at `_DAT_80089118..20` - and the overlay drives
//! them from the minigame driver `FUN_801CF3BC`, the lead's tick
//! `FUN_801D2278`, the lure tick `FUN_801D26CC` and the line actor
//! `FUN_801D4948`. [`VenueCamera`] is those globals; a host builds its view
//! from them and never moves them itself.
//!
//! The order a cast runs them in:
//!
//! 1. [`VenueCamera::reset`] (`FUN_801D78C0`) at the run-loop init, state `10`;
//! 2. [`VenueCamera::lead_publish`] every idle frame, while the lead's
//!    `+0x10` bit `0x80000` is clear (`0x801D20DC..0x801D20F0`);
//! 3. [`VenueCamera::wind_up`] every frame of state `0xd`: the dolly in over
//!    the water;
//! 4. nothing while the power meter runs and the lure flies;
//! 5. [`VenueCamera::in_water`] every frame the lure is in the water (lure
//!    `+0x22 == 2`), through the wait and the fight;
//! 6. on a catch, the line actor's close-up of the landed fish
//!    ([`VenueCamera::celebrate_begin`], [`VenueCamera::celebrate_step`],
//!    [`VenueCamera::celebrate_end`]), then the showcase of the party
//!    ([`VenueCamera::showcase`], [`VenueCamera::showcase_tilt`]).
//!
//! A broken line or a missed strike writes nothing more: the in-water pose
//! holds until the next state `10`.

use super::FishCamera;

/// `TR.z` floor of the wind-up dolly (`slti v0,v0,0x514` at `0x801CFA80`).
pub const WIND_UP_TR_Z_FLOOR: i32 = 0x514;

/// `TR.y` floor of the wind-up dolly (`slti v0,v0,0x5dc` at `0x801CFAA4`).
pub const WIND_UP_TR_Y_FLOOR: i32 = 0x5DC;

/// Pitch cap of the wind-up tilt (`slti v0,v0,0x51` at `0x801CFAF8`).
pub const WIND_UP_PITCH_CAP: i16 = 0x50;

/// Radius of the wind-up's per-frame focus step (`addiu a1,zero,0x14` at
/// `0x801CFAB4`, the polar helper's radius).
pub const WIND_UP_FOCUS_STEP: i32 = 0x14;

/// Line record at and above which the in-water pitch is left alone
/// (`slti v0,a1,0x7d0` at `0x801D308C`).
pub const IN_WATER_PITCH_RECORD: i32 = 0x7D0;

/// Per-venue focus of the catch showcase, `(x, z)` as the globals hold them
/// (`0x801CFDEC..0x801CFE0C`): venue `0` `(-0x1280, -0x2A00)`, venue `1`
/// `(-0x1700, -0x1200)` - the negated lead seat, as
/// [`VenueCamera::lead_publish`] writes it.
pub const SHOWCASE_FOCUS: [(i32, i32); 2] = [(-0x1280, -0x2A00), (-0x1700, -0x1200)];

/// The catch showcase's pose (`0x801CFE1C..0x801CFE48`): pitch `-0x208`,
/// yaw and roll `0`, `TR = (0, 0x706, -0x654)`.
pub const SHOWCASE_PITCH: i16 = -0x208;
/// See [`SHOWCASE_PITCH`].
pub const SHOWCASE_TR: [i32; 3] = [0, 0x706, -0x654];

/// Pitch the showcase tilt stops at (`slti v0,v0,-0x27` /
/// `addiu a0,zero,-0x28` at `0x801CFEB0..0x801CFEC8`).
pub const SHOWCASE_PITCH_END: i16 = -0x28;

/// The camera globals the line actor saves at the start of the catch
/// close-up and restores at its end (`0x801D4B2C..0x801D4BB4`,
/// `0x801D5174..0x801D51FC`): the whole `TR` quad, the pitch / yaw word, the
/// roll word and the whole focus quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Saved {
    rot: [i16; 3],
    tr: [i32; 3],
    focus: [i32; 3],
}

/// The fishing camera's retail globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueCamera {
    /// `_DAT_8007B790` / `_DAT_8007B792` / `_DAT_8007B794`: pitch, yaw, roll.
    pub rot: [i16; 3],
    /// `_DAT_800840B8` / `BC` / `C0`: the eye translation `TR`.
    pub tr: [i32; 3],
    /// `_DAT_80089118` / `1C` / `20`: the focus translation (the negated
    /// world point the view centres on).
    pub focus: [i32; 3],
    /// The lead's tick still publishes its camera (`+0x10` bit `0x80000`
    /// clear): set by [`Self::reset`], cleared by the cast press.
    pub lead_drives: bool,
    saved: Option<Saved>,
}

impl Default for VenueCamera {
    fn default() -> Self {
        let mut c = Self {
            rot: [0; 3],
            tr: [0; 3],
            focus: [0; 3],
            lead_drives: true,
            saved: None,
        };
        c.reset();
        c
    }
}

impl VenueCamera {
    /// The run-loop init's camera reset (`FUN_801D78C0`, called from state
    /// `10`): the rotation trio and `TR.x` zeroed, `TR.z` parked at
    /// [`crate::fishing_chrome::VENUE_CAMERA_TR_Z`]; `TR.y` and the focus are
    /// left for the lead's next publish. The same state clears the lead's
    /// `0x80000` bit (`0xFFF7FFFF` mask), handing the camera back to him.
    ///
    /// PORT: FUN_801d78c0
    pub fn reset(&mut self) {
        let r = crate::fishing_chrome::venue_camera_reset();
        self.rot = r.rot;
        self.tr[0] = r.tr_x;
        self.tr[2] = r.tr_z;
        self.lead_drives = true;
        self.saved = None;
    }

    /// The lead's camera publish (`FUN_801D2278`): yaw, `TR.y` and the focus
    /// off his seat and aim ([`super::fish_camera`]). Ignored once the cast
    /// press has taken the camera ([`Self::cast_press`]): the lead's tick
    /// `FUN_801D2050` skips the call while his `+0x10` bit `0x80000` is set
    /// (`lui v1,8` / `and` / `bnez` at `0x801D20E0..0x801D20E8`).
    ///
    /// PORT: FUN_801d2278 (camera publish)
    pub fn lead_publish(&mut self, cam: FishCamera) {
        if !self.lead_drives {
            return;
        }
        self.rot[1] = cam.yaw;
        self.tr[1] = cam.pitch_term;
        self.focus = [cam.translation.0, cam.translation.1, cam.translation.2];
    }

    /// The cast press in state `0xc` (`lui v1,8` / `or` / `sw` at
    /// `0x801CF9CC..0x801CF9D8`): the lead's `+0x10` gains
    /// `0x80000`, so his tick stops publishing.
    pub fn cast_press(&mut self) {
        self.lead_drives = false;
    }

    /// One frame of the cast wind-up, state `0xd` (`0x801CFA18..0x801CFB2C`),
    /// at frame step `step`, the lead facing `facing`:
    ///
    /// - `TR.z -= step << 7`, floored at [`WIND_UP_TR_Z_FLOOR`];
    /// - `TR.y -= step << 6`, floored at [`WIND_UP_TR_Y_FLOOR`] (a venue
    ///   whose idle `TR.y` sits under the floor jumps up to it);
    /// - pitch `+= step << 3`, capped at [`WIND_UP_PITCH_CAP`];
    /// - the focus steps out over the water by the polar offset of the facing
    ///   at radius [`WIND_UP_FOCUS_STEP`], scaled by `step` (`FUN_801D7BB8`).
    ///
    /// The camera closes in on the water ahead of the party, which leaves the
    /// shore view.
    ///
    /// PORT: FUN_801cf3bc (case 0xd - the wind-up camera)
    pub fn wind_up(&mut self, facing: i16, step: i32) {
        let step = step.max(1);
        self.tr[2] = (self.tr[2] - (step << 7)).max(WIND_UP_TR_Z_FLOOR);
        self.tr[1] = (self.tr[1] - (step << 6)).max(WIND_UP_TR_Y_FLOOR);
        let (sin, cos) = legaia_engine_field::minigame_floor::polar_tables();
        if let Some((dx, dz)) = legaia_engine_field::minigame_floor::polar_offset(
            facing as u32,
            WIND_UP_FOCUS_STEP,
            step,
            sin,
            cos,
        ) {
            self.focus[0] = self.focus[0].wrapping_add(dx);
            self.focus[2] = self.focus[2].wrapping_add(dz);
        }
        let p = self.rot[0].wrapping_add((step << 3) as i16);
        self.rot[0] = if p > WIND_UP_PITCH_CAP {
            WIND_UP_PITCH_CAP
        } else {
            p
        };
    }

    /// One in-water frame of the lure tick (lure `+0x22 == 2`, `FUN_801D26CC`):
    ///
    /// - **yaw** (`0x801D29E8..0x801D2A80`): with `aim = (facing + 0x800) &
    ///   0xFFF` and `d` the wrapped difference `aim - heading` (into
    ///   `-0x800..=0x800`), `yaw = d / 4 - aim`, the division truncating -
    ///   the lead's yaw turned a quarter of the way toward the lure.
    ///   `heading` is the bearing from the cast origin to the lure
    ///   (`FUN_80019B28` over `0x801D918C` and `0x801D9184`);
    /// - **pitch** (`0x801D3084..0x801D3158`), only while the line record is
    ///   under [`IN_WATER_PITCH_RECORD`]: the target is
    ///   `0x50 + (0x7D0 - record) * 0x17E / 2000` (a signed divide by the
    ///   reciprocal `0x10624DD3`), and the pitch moves toward it by
    ///   `step << 3`, clamped on the target. A fish reeled in tips the view
    ///   down toward the shore water.
    ///
    /// The `minigame_fishing_retail_lure_cast` state reads record `349`,
    /// pitch `0x18B`, yaw `0` with the lure straight out.
    ///
    /// PORT: FUN_801d26cc (the in-water camera writes)
    pub fn in_water(&mut self, facing: i16, heading: u16, record: i32, step: i32) {
        let step = step.max(1);
        let aim = (i32::from(facing) + 0x800) & 0xFFF;
        let h = i32::from(heading) & 0xFFF;
        let mut d = aim - h;
        if d > 0x800 {
            d = (aim - 0x1000) - h;
        }
        if d < -0x800 {
            d = aim - (h - 0x1000);
        }
        self.rot[1] = (d / 4 - aim) as i16;
        if record < IN_WATER_PITCH_RECORD {
            let target = (0x50 + (IN_WATER_PITCH_RECORD - record) * 0x17E / 2000) as i16;
            let delta = (step << 3) as i16;
            let p = self.rot[0];
            if p < target {
                self.rot[0] = p.wrapping_add(delta).min(target);
            } else if p > target {
                self.rot[0] = p.wrapping_sub(delta).max(target);
            }
        }
    }

    /// The first frame of the catch close-up (line actor sub-state `4`,
    /// `+0x22 == 0`, `0x801D4B24..0x801D4BF4`): save the globals, centre the
    /// focus on the landed fish at world `(x, z)` (its height untouched),
    /// zero `TR.x` / `TR.y` and the pitch, and push `TR.z` to `0x1000`. The
    /// yaw and roll carry over.
    ///
    /// PORT: FUN_801d4948 (case 4 - the close-up's save and set)
    pub fn celebrate_begin(&mut self, x: i16, z: i16) {
        self.saved = Some(Saved {
            rot: self.rot,
            tr: self.tr,
            focus: self.focus,
        });
        self.focus[0] = -i32::from(x);
        self.focus[2] = -i32::from(z);
        self.tr = [0, 0, 0x1000];
        self.rot[0] = 0;
    }

    /// One close-up frame (`0x801D4D98..0x801D4E20`), the fish's lifted
    /// height `y` (the line actor's `+0x16`): `TR.y = 0x200 - 4 y`,
    /// `TR.z -= step << 4`, pitch `-= step << 2`, yaw `-= 14 step` - the view
    /// swings round the fish as it is pulled from the water.
    ///
    /// PORT: FUN_801d4948 (case 4 - the close-up's orbit)
    pub fn celebrate_step(&mut self, y: i16, step: i32) {
        let step = step.max(1);
        self.tr[1] = 0x200 - 4 * i32::from(y);
        self.tr[2] -= step << 4;
        self.rot[1] = self.rot[1].wrapping_sub((14 * step) as i16);
        self.rot[0] = self.rot[0].wrapping_sub((step << 2) as i16);
    }

    /// The close-up's end (`+0x22 > 0xD2`, `0x801D5174..0x801D51FC`): the
    /// saved globals come back.
    ///
    /// PORT: FUN_801d4948 (case 4 - the close-up's restore)
    pub fn celebrate_end(&mut self) {
        if let Some(s) = self.saved.take() {
            self.rot = s.rot;
            self.tr = s.tr;
            self.focus = s.focus;
        }
    }

    /// The catch showcase, state `0x20` (`0x801CFDD8..0x801CFE48`): a fixed
    /// shot of the party from across the water - the venue's focus
    /// ([`SHOWCASE_FOCUS`]) at height `0`, pitch [`SHOWCASE_PITCH`], yaw and
    /// roll `0`, `TR` [`SHOWCASE_TR`].
    ///
    /// PORT: FUN_801cf3bc (case 0x20 - the showcase camera)
    pub fn showcase(&mut self, venue: usize) {
        let (fx, fz) = SHOWCASE_FOCUS[venue & 1];
        self.focus = [fx, 0, fz];
        self.rot = [SHOWCASE_PITCH, 0, 0];
        self.tr = SHOWCASE_TR;
    }

    /// One frame of the showcase tilt, state `0x21`
    /// (`0x801CFE70..0x801CFED0`): pitch `+= 5 step` until it reaches
    /// [`SHOWCASE_PITCH_END`], where it is pinned. Returns `true` on the frame
    /// it lands (the driver steps to `0x22`).
    ///
    /// PORT: FUN_801cf3bc (case 0x21 - the showcase tilt)
    pub fn showcase_tilt(&mut self, step: i32) -> bool {
        let p = self.rot[0].wrapping_add((5 * step.max(1)) as i16);
        if p < SHOWCASE_PITCH_END + 1 {
            self.rot[0] = p;
            false
        } else {
            self.rot[0] = SHOWCASE_PITCH_END;
            true
        }
    }
}

/// The landed fish's height through the catch close-up: the line actor's
/// `+0x16`, which it copies from the lure's 24.8 height accumulator
/// `0x801D9178` at the top of every tick (`0x801D4948..`), while sub-state `4`
/// raises the accumulator by `((rise << 8) / 20) * step` a frame (the
/// reciprocal `0x66666667` at `0x801D5214..`), `rise` (`+0x3E`) seeded at
/// `-0x300` on the first frame (`0x801D4B8C` / `0x801D4BC0`) and gaining
/// `5 * step` before each use (`0x801D4E28..0x801D4E34`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatchLift {
    /// `0x801D9178`, 24.8.
    pub acc: i32,
    /// `+0x3E`.
    pub rise: i16,
}

impl CatchLift {
    /// The lift for a lure caught at line record `record`. Retail holds the
    /// lure's height at or under `(record - 0xFA) << 8` once the record is
    /// under `400` (`0x801D2F50..0x801D2F60`), and a catch needs a record under
    /// `0x136`, so the port starts the lift at that ceiling (floored at the
    /// surface, `0`) - the lure's own sink below it is not modelled.
    pub fn at_record(record: i32) -> Self {
        Self {
            acc: ((record - 0xFA) << 8).max(0),
            rise: -0x300,
        }
    }

    /// The height the line actor reads this frame (`acc >> 8`).
    pub fn y(&self) -> i16 {
        (self.acc >> 8) as i16
    }

    /// Advance one close-up frame.
    pub fn step(&mut self, step: i32) {
        let step = step.max(1);
        self.rise = self.rise.wrapping_add((5 * step) as i16);
        self.acc += ((i32::from(self.rise) << 8) / 20) * step;
    }
}

/// One venue frame's inputs to [`VenueCameraDriver::frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueCameraFrame {
    /// The session's phase after this frame's tick.
    pub phase: crate::fishing::PondPhase,
    /// The lead's seat `(x, y, z)` and facing `+0x26` (his D-pad aim).
    pub lead: (i16, i16, i16, i16),
    /// The live line record (`0x801D927C`).
    pub record: i32,
    /// The lure's sideways walk-grid drift off its cast spawn, world units.
    pub lure_drift: i32,
    /// The venue (`DAT_801D90D0`), for the showcase's focus.
    pub venue: usize,
    /// The line actor's close-up timer `+0x22` as this frame starts, while
    /// it runs the catch close-up (sub-state `4`); `None` otherwise.
    pub close_up: Option<i16>,
    /// The close-up ends this frame (`+0x22 > 0xD2`).
    pub close_up_done: bool,
    /// The frame step (`0x1F800393`).
    pub step: i32,
}

/// Where the catch sequence stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Catch {
    #[default]
    None,
    /// The line actor's close-up of the landed fish, lifted from world
    /// `(x, z)`; the last field is the height this frame drew it at.
    CloseUp(CatchLift, (i16, i16), i16),
    /// The close-up is over; the next frame cuts to the showcase.
    Restored,
    /// State `0x21`: the showcase tilting up.
    Tilt,
    /// State `0x22`: the showcase held for the result.
    Held,
}

/// The fishing camera across the session's phases: which retail write site
/// runs on each frame, over one [`VenueCamera`]. Every host that runs the
/// venue frame drives one and builds its view from [`Self::camera`].
///
/// The session's phases map onto the driver's states: `Idle` is state `0xc`,
/// `WindUp` is `0xd` (the frame the session enters it is the cast press;
/// retail's thirteenth wind-up frame is the first `Power` frame, which the
/// driver's state `0xd` falls through into), `Flight` the flight, `Waiting` /
/// `Hooked` the lure in the water, `Landed` the catch, `Snapped` the break.
/// Re-entering `Idle` is state `10`'s reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VenueCameraDriver {
    /// The globals.
    pub camera: VenueCamera,
    prev: Option<crate::fishing::PondPhase>,
    catch: Catch,
}

impl VenueCameraDriver {
    /// Run one frame.
    pub fn frame(&mut self, f: VenueCameraFrame) {
        use crate::fishing::PondPhase as P;
        let (lx, ly, lz, facing) = f.lead;
        let step = f.step.max(1);
        let entered = self.prev != Some(f.phase);
        match f.phase {
            // The no-lure screen is state `10`'s shore with the gate parked.
            P::Idle | P::NoLure => {
                if entered {
                    self.camera.reset();
                    self.catch = Catch::None;
                }
                self.camera
                    .lead_publish(super::fish_camera(lx, ly, lz, facing));
            }
            P::WindUp => {
                if entered {
                    self.camera.cast_press();
                } else {
                    self.camera.wind_up(facing, step);
                }
            }
            P::Power => {
                if self.prev == Some(P::WindUp) {
                    self.camera.wind_up(facing, step);
                }
            }
            P::Flight | P::Snapped => {}
            P::Waiting | P::Hooked => {
                let heading = lure_heading(lx, lz, facing, f.record, f.lure_drift);
                self.camera.in_water(facing, heading, f.record, step);
            }
            P::Landed => self.landed(f, step),
        }
        self.prev = Some(f.phase);
    }

    /// The camera globals this frame.
    pub fn camera(&self) -> &VenueCamera {
        &self.camera
    }

    /// The landed fish's world point `(x, y, z)` while the close-up lifts it
    /// (the line actor's `+0x14..+0x18`, which it copies from the lure's
    /// 24.8 accumulators `0x801D9174..0x801D917C` at the top of every tick,
    /// `0x801D4954..0x801D499C`); `None` outside the close-up. The height is
    /// the one this frame's draw reads: the accumulator as this tick started
    /// (`0` on the first frame, the `sh zero,0x16` at `0x801D4BDC`), before
    /// the tick's own rise.
    pub fn lift_point(&self) -> Option<(i16, i16, i16)> {
        match self.catch {
            Catch::CloseUp(_, (x, z), y) => Some((x, y, z)),
            _ => None,
        }
    }

    fn landed(&mut self, f: VenueCameraFrame, step: i32) {
        let (lx, _, lz, facing) = f.lead;
        match (self.catch, f.close_up) {
            (Catch::None, Some(_)) => {
                let (x, z) = lure_world(lx, lz, facing, f.record, f.lure_drift);
                self.camera.celebrate_begin(x, z);
                // The first frame's height is the `sh zero,0x16` the save
                // block stores before the `TR.y` write reads it.
                self.camera.celebrate_step(0, step);
                let mut lift = CatchLift::at_record(f.record);
                lift.step(step);
                self.catch = Catch::CloseUp(lift, (x, z), 0);
            }
            (Catch::CloseUp(mut lift, at, _), _) => {
                let y = lift.y();
                self.camera.celebrate_step(y, step);
                lift.step(step);
                self.catch = Catch::CloseUp(lift, at, y);
            }
            (Catch::None, _) => {
                // No close-up ran (no line actor): straight to the showcase.
                self.catch = Catch::Restored;
            }
            (Catch::Restored, _) => {
                self.camera.showcase(f.venue);
                self.catch = Catch::Tilt;
            }
            (Catch::Tilt, _) => {
                if self.camera.showcase_tilt(step) {
                    self.catch = Catch::Held;
                }
            }
            (Catch::Held, _) => {}
        }
        if f.close_up_done && matches!(self.catch, Catch::CloseUp(..)) {
            self.camera.celebrate_end();
            self.catch = Catch::Restored;
        }
    }
}

/// The cast origin the lure is measured from: the lead's seat less the polar
/// offset of his facing at the lure spawn radius (`0x801CFCC4..0x801CFCD4`).
fn cast_origin(lx: i16, lz: i16, facing: i16) -> (i32, i32) {
    let (sin, cos) = legaia_engine_field::minigame_floor::polar_tables();
    let (dx, dz) = legaia_engine_field::minigame_floor::polar_offset(
        facing as u32,
        super::LURE_CAST_RADIUS,
        1,
        sin,
        cos,
    )
    .unwrap_or((0, 0));
    (i32::from(lx) - dx, i32::from(lz) - dz)
}

/// The lure's world point: `record` units out from the cast origin along the
/// facing (retail's live lure `0x801D9184` sits the line record from the
/// origin `0x801D918C` - the `minigame_fishing_retail_lure_cast` state reads
/// `349` for both), plus its sideways drift.
fn lure_world(lx: i16, lz: i16, facing: i16, record: i32, drift: i32) -> (i16, i16) {
    let (ox, oz) = cast_origin(lx, lz, facing);
    let (sin, cos) = legaia_engine_field::minigame_floor::polar_tables();
    let (dx, dz) = legaia_engine_field::minigame_floor::polar_offset(
        facing as u32,
        record.max(0),
        1,
        sin,
        cos,
    )
    .unwrap_or((0, 0));
    ((ox - dx + drift) as i16, (oz - dz) as i16)
}

/// The bearing from the cast origin to the lure, as the lure tick takes it
/// (`FUN_80019B28(origin.z, origin.x, lure.z, lure.x)`, `0x801D29F0..0x801D2A08`).
fn lure_heading(lx: i16, lz: i16, facing: i16, record: i32, drift: i32) -> u16 {
    let (ox, oz) = cast_origin(lx, lz, facing);
    let (x, z) = lure_world(lx, lz, facing, record, drift);
    legaia_engine_field::camera_zone::bearing(oz, ox, i32::from(z), i32::from(x)) & 0xFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The idle camera over the `minigame_fishing_retail_lure_cast` venue
    /// (venue `1`, lead at `(5888, -64, 4480)`, facing `0x800`).
    fn idle_venue1() -> VenueCamera {
        let mut c = VenueCamera::default();
        c.lead_publish(super::super::fish_camera(5888, -64, 4480, 0x800));
        c
    }

    fn frame(phase: crate::fishing::PondPhase) -> VenueCameraFrame {
        VenueCameraFrame {
            phase,
            lead: (5888, -64, 4480, 0x800),
            record: 349,
            lure_drift: 0,
            venue: 1,
            close_up: None,
            close_up_done: false,
            step: 1,
        }
    }

    #[test]
    fn the_driver_runs_the_cast_onto_the_captured_in_water_pose() {
        use crate::fishing::PondPhase as P;
        let mut d = VenueCameraDriver::default();
        d.frame(frame(P::Idle));
        assert_eq!(d.camera().tr, [0, 0x580, 0x974]);
        // The press frame takes the camera; twelve wind-up frames and the
        // power meter's first frame make retail's thirteen.
        for _ in 0..crate::fishing::WINDUP_FRAMES {
            d.frame(frame(P::WindUp));
        }
        d.frame(frame(P::Power));
        let wound = *d.camera();
        assert_eq!(wound.tr, [0, WIND_UP_TR_Y_FLOOR, WIND_UP_TR_Z_FLOOR]);
        assert_eq!(wound.focus[2], -0x1180 - 13 * 0x14);
        // The meter and the flight leave it alone.
        d.frame(frame(P::Power));
        d.frame(frame(P::Flight));
        assert_eq!(*d.camera(), wound);
        // In the water: the capture's pose (yaw 0 with the lure straight
        // out, pitch 0x18B at record 349).
        for _ in 0..100 {
            d.frame(frame(P::Waiting));
        }
        assert_eq!(d.camera().rot, [0x18B, 0, 0]);
        // A recast resets to the shore view.
        d.frame(frame(P::Idle));
        assert_eq!(d.camera().tr, [0, 0x580, 0x974]);
        assert_eq!(d.camera().rot, [0, 0, 0]);
    }

    #[test]
    fn the_driver_closes_up_restores_and_showcases_a_catch() {
        use crate::fishing::PondPhase as P;
        let mut d = VenueCameraDriver::default();
        d.frame(frame(P::Idle));
        d.frame(frame(P::Hooked));
        let fighting = *d.camera();
        let mut t = 0i16;
        loop {
            let done = t > 0xD2;
            d.frame(VenueCameraFrame {
                close_up: Some(t),
                close_up_done: done,
                record: 0x130,
                ..frame(P::Landed)
            });
            if t == 0 {
                assert_eq!(d.camera().tr[2], 0x1000 - 0x10);
            }
            if done {
                break;
            }
            t += 1;
        }
        assert_eq!(
            *d.camera(),
            fighting,
            "the close-up restores the fight pose"
        );
        d.frame(frame(P::Landed));
        assert_eq!(d.camera().tr, SHOWCASE_TR);
        assert_eq!(d.camera().focus, [-0x1700, 0, -0x1200]);
        for _ in 0..200 {
            d.frame(frame(P::Landed));
        }
        assert_eq!(d.camera().rot[0], SHOWCASE_PITCH_END);
    }

    #[test]
    fn the_idle_camera_is_the_reset_plus_the_lead_publish() {
        // `minigame_fishing_retail_ready`: rot 0, TR (0, 0x580, 0x974),
        // focus (-0x1700, 0, -0x1180).
        let c = idle_venue1();
        assert_eq!(c.rot, [0, 0, 0]);
        assert_eq!(c.tr, [0, 0x580, 0x974]);
        assert_eq!(c.focus, [-0x1700, 0, -0x1180]);
    }

    #[test]
    fn the_wind_up_dollies_onto_the_capture_pose() {
        // Thirteen frames at step 1, then the pitch the capture's record
        // tips the in-water view to.
        let mut c = idle_venue1();
        c.cast_press();
        for _ in 0..13 {
            c.wind_up(0x800, 1);
        }
        assert_eq!(c.tr, [0, WIND_UP_TR_Y_FLOOR, WIND_UP_TR_Z_FLOOR]);
        assert_eq!(c.rot[0], WIND_UP_PITCH_CAP);
        // The focus walked 13 * 0x14 world units out over the water (+Z).
        assert_eq!(c.focus, [-0x1700, 0, -0x1180 - 13 * 0x14]);
        // The lead's publish no longer lands.
        c.lead_publish(super::super::fish_camera(5888, -64, 4480, 0x800));
        assert_eq!(c.tr[1], WIND_UP_TR_Y_FLOOR);
    }

    #[test]
    fn the_in_water_pitch_converges_on_the_captured_target() {
        // `minigame_fishing_retail_lure_cast`: record 349, lure straight out
        // (heading 0 off facing 0x800), pitch 0x18B, yaw 0.
        let mut c = idle_venue1();
        c.cast_press();
        for _ in 0..13 {
            c.wind_up(0x800, 1);
        }
        for _ in 0..200 {
            c.in_water(0x800, 0, 349, 1);
        }
        assert_eq!(c.rot, [0x18B, 0, 0]);
        // At or past the record cut-off the pitch holds.
        c.in_water(0x800, 0, IN_WATER_PITCH_RECORD, 1);
        assert_eq!(c.rot[0], 0x18B);
    }

    #[test]
    fn the_in_water_yaw_turns_a_quarter_toward_the_lure() {
        let mut c = VenueCamera::default();
        // Lure 0x100 to the side of straight ahead: a quarter of the turn.
        c.in_water(0x800, 0xF00, 0x7D0, 1);
        assert_eq!(c.rot[1], 0x40);
        c.in_water(0x800, 0x100, 0x7D0, 1);
        assert_eq!(c.rot[1], -0x40);
        // Aim 0x900 (facing 0x100 past the water): yaw is the lead's.
        c.in_water(0x100, 0x900, 0x7D0, 1);
        assert_eq!(c.rot[1], -0x900);
    }

    #[test]
    fn the_close_up_saves_orbits_and_restores() {
        let mut c = idle_venue1();
        let before = c;
        c.celebrate_begin(5888, 5000);
        assert_eq!(c.focus, [-5888, 0, -5000]);
        assert_eq!(c.tr, [0, 0, 0x1000]);
        c.celebrate_step(0, 1);
        assert_eq!(c.tr, [0, 0x200, 0x1000 - 0x10]);
        assert_eq!(c.rot, [-4, -14, 0]);
        c.celebrate_end();
        assert_eq!(c, before);
    }

    #[test]
    fn the_showcase_tilts_up_to_its_end_pitch() {
        let mut c = VenueCamera::default();
        c.showcase(0);
        assert_eq!(c.focus, [-0x1280, 0, -0x2A00]);
        assert_eq!(c.tr, SHOWCASE_TR);
        let mut frames = 0;
        while !c.showcase_tilt(1) {
            frames += 1;
            assert!(frames < 200);
        }
        assert_eq!(c.rot[0], SHOWCASE_PITCH_END);
        // (-0x28 - -0x208) / 5 = 96 frames below the end, the 97th lands.
        assert_eq!(frames, 96);
    }

    #[test]
    fn the_catch_lift_rises_from_the_record_ceiling() {
        let mut l = CatchLift::at_record(0x130);
        assert_eq!(l.y(), 0x130 - 0xFA);
        l.step(1);
        // rise -0x2FB: (-0x2FB << 8) / 20 truncates to -9766.
        assert_eq!(l.acc, ((0x130 - 0xFA) << 8) - 9766);
        assert!(CatchLift::at_record(0x100).y() >= 0);
    }
}
