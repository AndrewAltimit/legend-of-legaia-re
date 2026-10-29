//! Action movement: an opt-in, platformer-style field controller - run with
//! momentum, sprint, jump, double jump, dash, air control, coyote time and
//! jump buffering, with gravity and landing on the scene's floor levels.
//!
//! Not retail. Retail's field walk (`FUN_801d01b0`, `engine-core`'s
//! `World::step_field_locomotion`) moves the player at a constant step with
//! no inertia and leaves height to a separate settle controller that glides
//! or hops; this model replaces both while its switch is on, for the
//! expansion modes that want a faster, more responsive body (prop hunt,
//! racing, co-op roaming) and as a play enhancement.
//!
//! Every quantity is a fixed-point integer - positions and velocities in
//! [`SUB`]-ths of a world unit, directions in q3.12 off the retail sine
//! table ([`legaia_render_kernels::gte::psx_sin`]) - so one input stream
//! gives one trajectory on every host, which lockstep netplay needs.
//!
//! The kernel is `World`-free: it reads walls and floors through
//! [`ActionTerrain`], which `engine-core` implements over its walk grid,
//! its placed-prop and NPC probes and `FUN_80019278`'s floor sample. World
//! Y grows **downward** (PSX convention): a raised floor is a smaller Y, and
//! a jump's velocity starts negative.

use legaia_render_kernels::gte::{psx_cos, psx_sin};

/// Fixed-point scale of positions and velocities: `256` sub-units per world
/// unit.
pub const SUB: i32 = 256;

/// The stick magnitude a full deflection (or a held d-pad direction) reads,
/// in the PSX analog range.
pub const STICK_FULL: i32 = 127;

/// Stick deflections shorter than this read as no direction.
pub const STICK_DEADZONE: i32 = 24;

/// Tuning for [`ActionBody::step`]. Speeds are [`SUB`]-ths of a world unit
/// per frame, accelerations the same per frame squared, heights whole
/// world units; frames are 60 Hz sim ticks. For scale: retail walks 8
/// units a frame and runs 12, and a field tile is 128 units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionParams {
    /// Top ground speed with the stick fully deflected.
    pub run_speed: i32,
    /// Top ground speed while the sprint button is held.
    pub sprint_speed: i32,
    /// Ground acceleration toward the stick's target velocity.
    pub ground_accel: i32,
    /// Ground deceleration with the stick released.
    pub ground_decel: i32,
    /// Ground acceleration while the stick points against the current
    /// velocity - the quick turn.
    pub turn_accel: i32,
    /// Acceleration toward the target velocity in the air (air control).
    pub air_accel: i32,
    /// Deceleration in the air with the stick released.
    pub air_decel: i32,
    /// Upward speed a ground jump starts with.
    pub jump_impulse: i32,
    /// Upward speed an air jump starts with.
    pub air_jump_impulse: i32,
    /// Jumps available in the air before landing again (`1` = double jump).
    pub air_jumps: u8,
    /// Gravity while rising with the jump button held.
    pub gravity: i32,
    /// Gravity while falling, or rising after the button was let go.
    pub fall_gravity: i32,
    /// Upward speed kept, in 256ths, when the jump button is let go while
    /// rising - the variable jump height.
    pub jump_cut: i32,
    /// Fastest fall.
    pub terminal_velocity: i32,
    /// Speed of a dash.
    pub dash_speed: i32,
    /// Frames a dash lasts; gravity is suspended for its length.
    pub dash_frames: u8,
    /// Frames after a dash starts before another may start.
    pub dash_cooldown: u8,
    /// Dashes available in the air before landing again.
    pub air_dashes: u8,
    /// Frames after walking off a ledge in which a jump still counts as a
    /// ground jump.
    pub coyote_frames: u8,
    /// Frames a jump press is remembered before landing.
    pub jump_buffer_frames: u8,
    /// Highest floor rise, in world units, a grounded step climbs without
    /// jumping (a stair). Taller rises are ledges and block the walk.
    pub step_height: i32,
    /// Largest floor drop, in world units, a grounded step follows down; a
    /// deeper one leaves the ground and falls.
    pub snap_down: i32,
    /// How far, in world units, a ledge may stand above an airborne body's
    /// feet and still be climbed onto when moving into it.
    pub mantle: i32,
    /// Frames the landing state holds after touching down.
    pub land_frames: u8,
}

impl Default for ActionParams {
    /// Snappy at 60 Hz: twice retail's walk speed, a jump that clears a
    /// ~145-unit ledge (about one floor level), a double jump to ~250.
    fn default() -> Self {
        Self {
            run_speed: 16 * SUB,
            sprint_speed: 24 * SUB,
            ground_accel: 3 * SUB,
            ground_decel: 4 * SUB,
            turn_accel: 6 * SUB,
            air_accel: 2 * SUB,
            air_decel: 2 * SUB,
            jump_impulse: 27 * SUB,
            air_jump_impulse: 23 * SUB,
            air_jumps: 1,
            gravity: 5 * SUB / 2,
            fall_gravity: 13 * SUB / 4,
            jump_cut: SUB / 2,
            terminal_velocity: 36 * SUB,
            dash_speed: 36 * SUB,
            dash_frames: 9,
            dash_cooldown: 30,
            air_dashes: 1,
            coyote_frames: 6,
            jump_buffer_frames: 6,
            step_height: 48,
            snap_down: 48,
            mantle: 40,
            land_frames: 6,
        }
    }
}

/// One frame's controls, camera-relative.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionInput {
    /// Screen-space stick: `x` right, `y` **up**, each `-127..=127`. A
    /// d-pad direction is a full deflection.
    pub stick: (i32, i32),
    /// Camera azimuth, PSX 12-bit angle: `0` maps screen-up to world `+Z`.
    pub camera_azimuth: u16,
    /// The jump button is down this frame.
    pub jump_held: bool,
    /// The jump button went down this frame.
    pub jump_pressed: bool,
    /// The dash button went down this frame.
    pub dash_pressed: bool,
    /// The sprint button is down this frame.
    pub sprint_held: bool,
}

/// What the body is doing, for the host's clip pick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ActionAnim {
    /// Grounded and still.
    #[default]
    Idle,
    /// Grounded and moving.
    Run,
    /// Airborne and rising.
    Jump,
    /// Airborne and falling.
    Fall,
    /// The frames right after touching down.
    Land,
    /// A dash in progress.
    Dash,
}

/// Walls, bodies and floors as the kernel sees them.
pub trait ActionTerrain {
    /// Whether a one-unit step from `(x, z)` along `dir` is refused by a
    /// wall or a solid body. `dir` is retail's probe index: `0` = Z-,
    /// `1` = X-, `2` = Z+, `3` = X+.
    fn step_blocked(&mut self, x: i16, z: i16, dir: usize) -> bool;
    /// The floor height under `(x, z)`, Y down.
    fn floor_y(&mut self, x: i32, z: i32) -> i32;
}

/// What one [`ActionBody::step`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActionStep {
    /// The body's whole-unit position changed.
    pub moved: bool,
    /// The stick held a direction this frame.
    pub steering: bool,
    /// A jump (ground, coyote, buffered or air) started this frame.
    pub jumped: bool,
    /// The body touched down this frame.
    pub landed: bool,
    /// A dash started this frame.
    pub dashed: bool,
    /// A wall or ledge refused the X / Z step this frame.
    pub blocked: (bool, bool),
    /// The retail direction mask (`0x1000` Z+, `0x4000` Z-, `0x2000` X+,
    /// `0x8000` X-): the stick's world direction on a steered frame, the
    /// committed travel otherwise.
    pub dir_bits: u16,
    /// The facing after the step, PSX 12-bit angle (`0` = +Z).
    pub heading: u16,
    /// The state for the clip pick.
    pub anim: ActionAnim,
}

/// The controller's state: one body. Positions are absolute, in [`SUB`]-ths
/// of a world unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ActionBody {
    /// `(x, y, z)`, Y down.
    pub pos: (i32, i32, i32),
    /// Horizontal velocity `(x, z)`.
    pub vel: (i32, i32),
    /// Vertical velocity, positive falling.
    pub vy: i32,
    /// Standing on the floor.
    pub grounded: bool,
    /// Frames of coyote time left after leaving the ground without jumping.
    pub coyote: u8,
    /// Frames a buffered jump press stays live.
    pub jump_buffer: u8,
    /// Air jumps spent since the last landing.
    pub air_jumps_used: u8,
    /// Air dashes spent since the last landing.
    pub air_dashes_used: u8,
    /// Frames of dash left.
    pub dash_timer: u8,
    /// Frames before another dash may start.
    pub dash_cooldown: u8,
    /// The dash's direction, q3.12.
    pub dash_dir: (i32, i32),
    /// The current jump's rise was already cut by a release.
    pub jump_cut_done: bool,
    /// Frames of the landing state left.
    pub land_timer: u8,
    /// Facing, PSX 12-bit angle.
    pub heading: u16,
}

impl ActionBody {
    /// A body standing at whole-unit `(x, y, z)` facing `heading`.
    pub fn at(x: i32, y: i32, z: i32, heading: u16) -> Self {
        Self {
            pos: (x * SUB, y * SUB, z * SUB),
            grounded: true,
            heading: heading & 0x0FFF,
            ..Self::default()
        }
    }

    /// The whole-unit position.
    pub fn unit_pos(&self) -> (i32, i32, i32) {
        (
            self.pos.0.div_euclid(SUB),
            self.pos.1.div_euclid(SUB),
            self.pos.2.div_euclid(SUB),
        )
    }

    /// Re-seat the body on `(x, y, z)` when something else moved the actor
    /// (a warp, a script); a body already there keeps its momentum and its
    /// sub-unit remainder. Returns whether it re-seated.
    pub fn sync_to(&mut self, x: i32, y: i32, z: i32, heading: u16) -> bool {
        if self.unit_pos() == (x, y, z) {
            return false;
        }
        *self = Self::at(x, y, z, heading);
        true
    }

    /// Whether the body is mid-air.
    pub fn airborne(&self) -> bool {
        !self.grounded
    }

    /// Advance one frame.
    pub fn step(
        &mut self,
        p: &ActionParams,
        input: &ActionInput,
        terrain: &mut impl ActionTerrain,
    ) -> ActionStep {
        let mut out = ActionStep::default();
        let start = self.unit_pos();
        self.dash_cooldown = self.dash_cooldown.saturating_sub(1);
        self.land_timer = self.land_timer.saturating_sub(1);
        if self.grounded {
            self.coyote = p.coyote_frames;
            self.air_jumps_used = 0;
            self.air_dashes_used = 0;
        } else {
            self.coyote = self.coyote.saturating_sub(1);
        }

        // Steering: rotate the screen stick into the world by the camera,
        // and cap its length at a full deflection so a keyboard diagonal is
        // no faster than a straight line.
        let (wx, wz, mag) = world_stick(input);
        let steering = mag > 0;
        out.steering = steering;

        // Jump: a fresh press in the air spends an air jump when one is
        // left; otherwise the press waits in the buffer for the ground.
        let mut jump_now = false;
        if input.jump_pressed {
            if !self.grounded && self.coyote == 0 && self.air_jumps_used < p.air_jumps {
                self.air_jumps_used += 1;
                self.vy = -p.air_jump_impulse;
                self.dash_timer = 0;
                self.jump_cut_done = false;
                out.jumped = true;
            } else {
                self.jump_buffer = p.jump_buffer_frames.max(1);
            }
        }
        if self.jump_buffer > 0 && (self.grounded || self.coyote > 0) {
            jump_now = true;
        }
        self.jump_buffer = self.jump_buffer.saturating_sub(1);
        if jump_now {
            self.vy = -p.jump_impulse;
            self.grounded = false;
            self.coyote = 0;
            self.jump_buffer = 0;
            self.dash_timer = 0;
            self.jump_cut_done = false;
            out.jumped = true;
        }
        // Variable height: letting go while rising cuts the rise once.
        if !input.jump_held && self.vy < 0 && !self.jump_cut_done && !self.grounded {
            self.vy = self.vy * p.jump_cut / SUB;
            self.jump_cut_done = true;
        }

        // Dash: along the stick, else along the facing.
        if input.dash_pressed
            && self.dash_cooldown == 0
            && (self.grounded || self.air_dashes_used < p.air_dashes)
        {
            self.dash_dir = if steering {
                (wx, wz)
            } else {
                (psx_sin(self.heading), psx_cos(self.heading))
            };
            self.dash_timer = p.dash_frames;
            self.dash_cooldown = p.dash_cooldown;
            if !self.grounded {
                self.air_dashes_used += 1;
            }
            out.dashed = true;
        }

        let dashing = self.dash_timer > 0;
        if dashing {
            self.dash_timer -= 1;
            self.vel = (
                self.dash_dir.0 * p.dash_speed / 4096,
                self.dash_dir.1 * p.dash_speed / 4096,
            );
            if !self.grounded {
                self.vy = 0;
            }
        } else {
            let top = if input.sprint_held {
                p.sprint_speed
            } else {
                p.run_speed
            };
            let target = (
                wx * top / 4096 * mag / STICK_FULL,
                wz * top / 4096 * mag / STICK_FULL,
            );
            let reversing = steering
                && (self.vel.0 as i64 * target.0 as i64 + self.vel.1 as i64 * target.1 as i64) < 0;
            let rate = match (self.grounded, steering) {
                (true, false) => p.ground_decel,
                (true, true) if reversing => p.turn_accel,
                (true, true) => p.ground_accel,
                (false, false) => p.air_decel,
                (false, true) => p.air_accel,
            };
            self.vel = approach(self.vel, target, rate);
        }

        // Gravity, suspended through a dash.
        if !self.grounded && !dashing {
            let g = if self.vy < 0 && input.jump_held && !self.jump_cut_done {
                p.gravity
            } else {
                p.fall_gravity
            };
            self.vy = (self.vy + g).min(p.terminal_velocity);
        }

        // Horizontal travel, one world unit at a time, Z before X.
        let (bx, bz) = self.travel(p, terrain, &mut out);
        out.blocked = (bx, bz);

        // Vertical travel and landing.
        if !self.grounded {
            self.pos.1 += self.vy;
            let (x, y, z) = self.unit_pos();
            let floor = terrain.floor_y(x, z);
            if self.vy >= 0 && y >= floor {
                self.land(floor, p, &mut out);
            } else if y > floor {
                // Rising through a floor that rose under the body.
                self.pos.1 = floor * SUB;
            }
        } else {
            // Standing: follow a floor that moved under the body (the field
            // VM's ladder ops), or start falling when it dropped away.
            let (x, y, z) = self.unit_pos();
            let floor = terrain.floor_y(x, z);
            if floor - y > p.snap_down {
                self.leave_ground();
            } else {
                self.pos.1 = floor * SUB;
            }
        }

        if steering {
            self.heading = heading_of(wx, wz);
        }
        let end = self.unit_pos();
        out.moved = end != start;
        // A steered frame reports the stick's direction even when a wall
        // refused it - the touch probes push into what the player pushes
        // into, as the retail walk's held mask does.
        out.dir_bits = if steering {
            dir_bits(wx, wz)
        } else {
            dir_bits(end.0 - start.0, end.2 - start.2)
        };
        out.heading = self.heading;
        out.anim = self.anim(steering, dashing);
        out
    }

    /// Walk the frame's horizontal velocity one unit at a time against the
    /// terrain. Returns which axes were refused.
    fn travel(
        &mut self,
        p: &ActionParams,
        terrain: &mut impl ActionTerrain,
        out: &mut ActionStep,
    ) -> (bool, bool) {
        let from = self.unit_pos();
        let target_x = (self.pos.0 + self.vel.0).div_euclid(SUB);
        let target_z = (self.pos.2 + self.vel.1).div_euclid(SUB);
        let (mut dx, mut dz) = (target_x - from.0, target_z - from.2);
        let (mut bx, mut bz) = (false, false);
        // The sub-unit remainders travel with the body; whole units are
        // committed below, one probe each.
        self.pos.0 += self.vel.0;
        self.pos.2 += self.vel.1;
        self.pos.0 = from.0 * SUB + self.pos.0.rem_euclid(SUB);
        self.pos.2 = from.2 * SUB + self.pos.2.rem_euclid(SUB);
        while dx != 0 || dz != 0 {
            if dz != 0 {
                let s = dz.signum();
                if self.try_unit(p, terrain, 0, s, if s > 0 { 2 } else { 0 }, out) {
                    dz -= s;
                } else {
                    bz = true;
                    dz = 0;
                    self.vel.1 = 0;
                }
            }
            if dx != 0 {
                let s = dx.signum();
                if self.try_unit(p, terrain, s, 0, if s > 0 { 3 } else { 1 }, out) {
                    dx -= s;
                } else {
                    bx = true;
                    dx = 0;
                    self.vel.0 = 0;
                }
            }
        }
        (bx, bz)
    }

    /// One whole-unit step along `(sx, sz)`. Walls refuse it outright; the
    /// floor ahead decides between climbing, following, leaving the ground
    /// and refusing a ledge.
    fn try_unit(
        &mut self,
        p: &ActionParams,
        terrain: &mut impl ActionTerrain,
        sx: i32,
        sz: i32,
        dir: usize,
        out: &mut ActionStep,
    ) -> bool {
        let (x, y, z) = self.unit_pos();
        if terrain.step_blocked(clamp16(x), clamp16(z), dir) {
            return false;
        }
        let (nx, nz) = (x + sx, z + sz);
        let floor = terrain.floor_y(nx, nz);
        // Rise above the feet, positive up (Y grows downward).
        let rise = y - floor;
        if self.grounded {
            if rise > p.step_height {
                return false;
            }
            if -rise > p.snap_down {
                self.leave_ground();
            } else {
                self.pos.1 = floor * SUB;
            }
        } else if rise > 0 {
            if rise > p.mantle {
                return false;
            }
            if self.vy >= 0 {
                self.commit(nx, nz);
                self.land(floor, p, out);
                return true;
            }
            // Rising past a lip: the feet ride up over it.
            self.pos.1 = floor * SUB;
        }
        self.commit(nx, nz);
        true
    }

    fn commit(&mut self, x: i32, z: i32) {
        self.pos.0 = x * SUB + self.pos.0.rem_euclid(SUB);
        self.pos.2 = z * SUB + self.pos.2.rem_euclid(SUB);
    }

    fn leave_ground(&mut self) {
        self.grounded = false;
        self.vy = self.vy.max(0);
        self.jump_cut_done = true;
    }

    fn land(&mut self, floor: i32, p: &ActionParams, out: &mut ActionStep) {
        self.pos.1 = floor * SUB;
        self.vy = 0;
        self.grounded = true;
        self.dash_timer = 0;
        self.air_jumps_used = 0;
        self.air_dashes_used = 0;
        self.land_timer = p.land_frames;
        self.coyote = p.coyote_frames;
        out.landed = true;
    }

    fn anim(&self, steering: bool, dashing: bool) -> ActionAnim {
        if dashing {
            ActionAnim::Dash
        } else if !self.grounded {
            if self.vy < 0 {
                ActionAnim::Jump
            } else {
                ActionAnim::Fall
            }
        } else if self.land_timer > 0 && !steering {
            ActionAnim::Land
        } else if self.vel != (0, 0) || steering {
            ActionAnim::Run
        } else {
            ActionAnim::Idle
        }
    }
}

/// The stick as a world-space q3.12 unit direction plus its magnitude
/// (`0..=STICK_FULL`, `0` inside the deadzone).
fn world_stick(input: &ActionInput) -> (i32, i32, i32) {
    let (sx, sy) = input.stick;
    let len = isqrt((sx as i64 * sx as i64 + sy as i64 * sy as i64) as u64) as i32;
    if len < STICK_DEADZONE {
        return (0, 0, 0);
    }
    let mag = len.min(STICK_FULL);
    // Unit screen vector in q3.12, then rotate by the camera: azimuth 0 is
    // the identity (screen-up = +Z, screen-right = +X).
    let ux = sx * 4096 / len;
    let uy = sy * 4096 / len;
    let (s, c) = (psx_sin(input.camera_azimuth), psx_cos(input.camera_azimuth));
    let wx = (ux * c + uy * s) >> 12;
    let wz = (-ux * s + uy * c) >> 12;
    (wx, wz, mag)
}

/// Move `v` toward `target` by at most `rate` (vector length).
fn approach(v: (i32, i32), target: (i32, i32), rate: i32) -> (i32, i32) {
    let (dx, dz) = (target.0 - v.0, target.1 - v.1);
    let len = isqrt((dx as i64 * dx as i64 + dz as i64 * dz as i64) as u64) as i64;
    if len <= rate as i64 {
        return target;
    }
    (
        v.0 + (dx as i64 * rate as i64 / len) as i32,
        v.1 + (dz as i64 * rate as i64 / len) as i32,
    )
}

/// Integer square root (floor).
pub fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = (n as f64).sqrt() as u64;
    // Correct the float seed so the result is exact for every input.
    while x * x > n {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= n {
        x += 1;
    }
    x
}

/// The PSX 12-bit heading of world direction `(x, z)`: `0` = +Z, `0x400` =
/// +X - the convention of the retail walk's `atan2(wx, wz)`. Integer
/// throughout: a binary search on the sine table inside the quadrant.
pub fn heading_of(x: i32, z: i32) -> u16 {
    if x == 0 && z == 0 {
        return 0;
    }
    let (ax, az) = (x.unsigned_abs() as i64, z.unsigned_abs() as i64);
    // Smallest a in 0..=0x400 with sin(a) * |z| >= cos(a) * |x|.
    let (mut lo, mut hi) = (0u16, 0x400u16);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if psx_sin(mid) as i64 * az >= psx_cos(mid) as i64 * ax {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    let a = lo;
    let h = match (x >= 0, z >= 0) {
        (true, true) => a,
        (true, false) => 0x800 - a,
        (false, false) => 0x800 + a,
        (false, true) => 0x1000 - a,
    };
    h & 0x0FFF
}

/// The retail direction mask of a committed `(dx, dz)`.
pub fn dir_bits(dx: i32, dz: i32) -> u16 {
    let mut bits = 0;
    if dz > 0 {
        bits |= 0x1000;
    } else if dz < 0 {
        bits |= 0x4000;
    }
    if dx > 0 {
        bits |= 0x2000;
    } else if dx < 0 {
        bits |= 0x8000;
    }
    bits
}

fn clamp16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(test)]
mod tests;
