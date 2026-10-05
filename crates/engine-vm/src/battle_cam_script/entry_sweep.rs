//! The battle-entry sweep: the camera move the SCUS frame driver runs before
//! the battle tick takes over.

use super::camera::Glide;
use super::*;

/// The pitch battle init seeds (`li v0,0x3c ; sh v0,-0x4870(at)` at
/// `0x80055E50..0x80055E60`, into `0x8007B790`).
pub const ENTRY_PITCH: f32 = 0x3C as f32;
/// The translation trio battle init seeds into `0x800840B8/BC/C0`
/// (`0x80055E64..0x80055E90`), already in the globals' prescaled units.
pub const ENTRY_TR: [f32; 3] = [0.0, 0x500 as f32, 0x1C00 as f32];
/// The entry counter `gp+0x330` (`0x8007B648`) when the sweep starts: the
/// frame driver loads the fight below it and sweeps from here.
pub const ENTRY_COUNTER_START: u32 = 0x80;
/// The counter value from which the frame driver re-arms case 2 instead of
/// drifting (`sltiu v0,v0,0xa2` at `0x80046F84`).
pub const ENTRY_CASE2_FROM: u32 = 0xA2;
/// The last counter value the sweep runs at: past it the driver parks the
/// counter at `0xFF` and calls the battle tick from then on
/// (`sltiu v0,v0,0xc1` at `0x80046FFC`).
pub const ENTRY_COUNTER_LAST: u32 = 0xC0;
/// The drift below [`ENTRY_CASE2_FROM`], per display frame: TR y `+0x30`
/// and TR z `-0x40` (`(fs * 3) << 4` / `fs << 6` at `0x80046FA4..0x80046FDC`).
pub const ENTRY_DRIFT_TR_Y: f32 = 0x30 as f32;
pub const ENTRY_DRIFT_TR_Z: f32 = 0x40 as f32;
/// Case 2's TR (`0x801D5BB0..0x801D5BD0`): `(0, 0x600, 0x700)`, raw z.
pub const ENTRY_CASE2_TR_Y: f32 = 0x600 as f32;
pub const ENTRY_CASE2_TR_Z_RAW: i32 = 0x700;
/// Case 2's tween duration (`li a3,0xc` at `0x801D5BBC`).
pub const ENTRY_CASE2_FRAMES: u32 = 0xC;

impl BattleCamera {
    /// Start the battle-entry sweep from battle init's seeded pose.
    ///
    /// Battle init `FUN_80055B6C` writes the camera globals - pitch `0x3C`,
    /// roll `0`, TR `(0, 0x500, 0x1C00)` (`0x80055E50..0x80055E90`); it also
    /// zeroes the yaw `0x8007B792`, which the port keeps at the inherited
    /// entry azimuth instead ([`battle_entry_yaw`]). The SCUS frame driver
    /// `FUN_80046A20` then owns the camera until its entry counter `gp+0x330`
    /// passes `0xC0` (`0x80046EEC..0x8004700C`), and only then calls the
    /// battle tick `FUN_801D0748`:
    ///
    /// - counter `0x80..0xA1`: TR y `+= 0x30 * fs`, TR z `-= 0x40 * fs` - the
    ///   camera rises and pulls in;
    /// - counter `0xA2..=0xC0`: `FUN_801D5854(0, 2)` every pass - case 2's
    ///   pitch `0`, yaw `0`, TR `(0, 0x600, 0x700)` on the origin over
    ///   `a3 = 0xC`, re-armed each pass, so the camera eases toward it;
    ///
    /// with the counter advancing by the frame step `fs` each pass. Two
    /// captures of the sparring fight's entry pin it:
    /// `v0_1_battle_loading_tetsu` (counter `0x84`) reads pitch `60`,
    /// TR `(0, 1472, 6912)` - four frames of drift - and `s5_tetsu_battle`
    /// (`0xAF`) pitch `16`, TR `(0, 2010, 3552)`, the case-2 step table
    /// armed against the origin.
    ///
    /// REF: FUN_80055B6C, FUN_80046A20, FUN_801D5854 (case 2)
    pub fn start_entry_sweep(&mut self) {
        self.pose = BattleCamPose {
            pitch: ENTRY_PITCH,
            yaw: self.pose.yaw,
            tr: ENTRY_TR,
            focus: [0.0; 3],
        };
        self.glides.clear();
        self.entry_sweep = Some(ENTRY_COUNTER_START);
    }

    /// The entry counter `gp+0x330` while the sweep runs, `None` once the
    /// battle tick owns the camera.
    pub fn entry_sweep_counter(&self) -> Option<u32> {
        self.entry_sweep
    }

    /// One camera step (two display frames) of the entry sweep. Returns
    /// `false` once the sweep is over.
    pub(super) fn step_entry_sweep(&mut self) -> bool {
        let Some(counter) = self.entry_sweep else {
            return false;
        };
        let fs = 2.0;
        if counter < ENTRY_CASE2_FROM {
            self.pose.tr[1] += ENTRY_DRIFT_TR_Y * fs;
            self.pose.tr[2] -= ENTRY_DRIFT_TR_Z * fs;
        } else {
            let target = BattleCamPose {
                pitch: 0.0,
                yaw: self.pose.yaw,
                tr: [0.0, ENTRY_CASE2_TR_Y, prescale_tr_z(ENTRY_CASE2_TR_Z_RAW)],
                focus: [0.0; 3],
            };
            let mut from = self.pose;
            let g = Glide::chase(&mut from, target, ENTRY_CASE2_TR_Z_RAW, ENTRY_CASE2_FRAMES);
            self.pose = from;
            self.step_components(&g);
            self.pose.yaw = self.pose.yaw.rem_euclid(4096.0);
        }
        let next = counter + 2;
        if next > ENTRY_COUNTER_LAST {
            self.entry_sweep = None;
            self.hand_over_from_entry_sweep();
        } else {
            self.entry_sweep = Some(next);
        }
        true
    }

    /// The battle tick's first framing after the sweep: the tutorial's
    /// dialogue close-up is a cut (PROT 0967's shot over `1` frame); any
    /// other opening re-arms case 9's far framing over its `a3 = 0xE` from
    /// wherever the sweep left the camera, with the orbit owning the yaw.
    fn hand_over_from_entry_sweep(&mut self) {
        self.glides.clear();
        match self.phase {
            BattleCamPhase::Dialogue => self.pose = dialogue_pose(self.formation),
            BattleCamPhase::Menu => {
                let mut from = self.pose;
                let g = Glide::linear(
                    &mut from,
                    self.menu_pose(),
                    menu_raw_z(self.formation),
                    SWING_RETURN_STEPS,
                    false,
                );
                self.pose = from;
                self.glides.push_back(g);
            }
            // An action already running is re-framed every pass.
            BattleCamPhase::Action | BattleCamPhase::Recover | BattleCamPhase::ActionEnd => {}
            p @ (BattleCamPhase::Submenu
            | BattleCamPhase::TargetEnemy
            | BattleCamPhase::TargetAlly) => {
                self.phase = BattleCamPhase::Menu;
                self.set_phase(p);
            }
        }
    }
}
