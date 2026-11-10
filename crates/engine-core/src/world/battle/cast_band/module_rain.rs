//! PROT 0920's (Slippery) **Deadly Rain**: the module words its arms write,
//! the per-frame routine the battle draw tick calls while the budget is
//! non-zero, and the streaks both hosts draw.

use super::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::cast_rain::{RainCamera, RainLine};

impl World {
    /// One call of PROT 0920's rain routine (`FUN_801F7B88`) - retail's
    /// per-actor battle draw tick makes it once a battle frame while the
    /// budget `_DAT_8007BDC0` is non-zero (`jal 0x801F7B88` at `0x800481A0`).
    /// The engine makes it once a battle frame after the module's own pass,
    /// at that frame's step, through the GTE state the draw tick leaves
    /// ([`RainCamera::battle`]) and on the world's one `rand()` stream. A
    /// call per vsync at rate `1` instead falls each drop a quarter as far
    /// per call at step `4` and rolls its spawns four times as often: the
    /// storm stood four times as dense as retail's.
    ///
    /// REF: FUN_801F7B88
    pub(in crate::world::battle::cast_band) fn step_module_rain(&mut self) {
        if self.casting.module_rain.budget == 0 {
            self.casting.module_rain.frame = Default::default();
            return;
        }
        let facing = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map_or(0, |a| a.battle.facing_angle & 0xFFF);
        let pose = self.battle_cam_pose();
        let i = |v: f32| v.round() as i32;
        let cam = RainCamera::battle(
            i(pose.pitch) as i16,
            i(pose.yaw) as i16,
            pose.tr.map(i),
            pose.focus.map(i),
        );
        // The call's rate is the frame step `*(0x1F800393)` the routine reads
        // (`lbu` of `0x1F800393` scaling the drift and the `r << 6` drop), and
        // its spawn rolls are one per free record per call: a frame of `step`
        // vsyncs makes one call. On retail's cadence this pass is that frame's
        // one; on the per-vsync cadence only the tick that opens a frame calls.
        let opens_frame = self.toggles.battle_sm_frame_cadence
            || self
                .battle
                .frame_clock
                .ticks_into_frame(self.clock.display_frames)
                == 0;
        if !opens_frame {
            return;
        }
        let rate = self.battle_frame_step().max(1);
        let mut rain = std::mem::take(&mut self.casting.module_rain);
        rain.frame = rain
            .drops
            .step(rain.budget, rate, facing, rain.colour, &cam, || {
                self.next_rand()
            });
        // The splashes are read off the frame just made, so the state goes
        // back first: spawned off the taken (default) state, none ever seated.
        self.casting.module_rain = rain;
        self.casting.spawn_rain_splashes();
    }

    /// The streaks PROT 0920's rain routine drew on its last call, which
    /// `legaia_render_kernels::cast_rain::rain_line_prims` turns into the
    /// frame's primitives on both hosts. Empty outside a battle, outside a
    /// live cast, or while the routine is not being called.
    pub fn module_rain_lines(&self) -> &[RainLine] {
        if self.mode != SceneMode::Battle
            || self.casting.summon_stager.is_none()
            || self.casting.module_rain.budget == 0
        {
            return &[];
        }
        &self.casting.module_rain.frame.lines
    }
}
