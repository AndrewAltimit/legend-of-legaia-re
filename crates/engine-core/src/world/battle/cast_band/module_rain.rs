//! PROT 0920's (Slippery) **Deadly Rain**: the module words its arms write,
//! the per-frame routine the battle draw tick calls while the budget is
//! non-zero, and the streaks both hosts draw.

use super::*;
use legaia_engine_cast::cast_module_camera::{
    DELTA_PER_TICK, SLIPPERY_COLOUR_ARM, SLIPPERY_RAIN_DRAIN_ARM, SLIPPERY_RAIN_SEED_ARM,
};
use legaia_engine_mode::mode::SceneMode;
use legaia_render_kernels::cast_rain::{RainCamera, RainLine};

/// The grey arm 0 stores in the colour word (`0x801F6BAC`).
const RAIN_GREY: u32 = 0x0080_8080;
/// The red arm 9's exit pins (`0x801F77B0`).
const RAIN_RED: u32 = 0x00FF_4040;
/// The budget arm 5's exit seeds (`0x801F6FEC`).
const RAIN_BUDGET_SEED: i32 = 0x204;
/// The floor arm 6's exit pins (`0x801F712C`).
const RAIN_BUDGET_FLOOR: i32 = 4;

impl World {
    /// The rain words PROT 0920's arm `phase_in` wrote on this pass;
    /// `passed` is whether the director let the arm through.
    pub(in crate::world::battle::cast_band) fn slippery_rain_words(
        &mut self,
        phase_in: u8,
        passed: bool,
    ) {
        let rain = &mut self.casting.module_rain;
        match phase_in {
            0 => {
                // `0x801F6B98..0x801F6BFC`: grey colour, zero budget, every
                // record freed.
                rain.colour = RAIN_GREY;
                rain.budget = 0;
                rain.drops.clear();
            }
            SLIPPERY_RAIN_SEED_ARM if passed => rain.budget = RAIN_BUDGET_SEED,
            SLIPPERY_RAIN_DRAIN_ARM => {
                // `budget += rate * -2` on every pass, before the gate.
                rain.budget -= 2 * DELTA_PER_TICK;
                if passed {
                    rain.budget = RAIN_BUDGET_FLOOR;
                }
            }
            SLIPPERY_COLOUR_ARM => {
                // One step a vsync: red up to `0xFF`, green and blue down
                // together until blue reads `0x40`.
                for _ in 0..DELTA_PER_TICK {
                    if rain.colour & 0xFF_0000 != 0xFF_0000 {
                        rain.colour = rain.colour.wrapping_add(0x1_0000);
                    }
                    if rain.colour & 0xFF != 0x40 {
                        rain.colour = rain.colour.wrapping_sub(0x101);
                    }
                }
                if passed {
                    rain.colour = RAIN_RED;
                }
            }
            legaia_engine_cast::seru_magic_ticks::SERU_B_DONE_PHASE => rain.budget = 0,
            _ => {}
        }
    }

    /// One call of PROT 0920's rain routine (`FUN_801F7B88`) - retail's
    /// per-actor battle draw tick makes it once a battle frame while the
    /// budget `_DAT_8007BDC0` is non-zero (`jal 0x801F7B88` at `0x800481A0`).
    /// The engine makes it once a tick after the module's own pass, at the
    /// frame step of one vsync the module's drains assume, through the GTE
    /// state the draw tick leaves ([`RainCamera::battle`]) and on the world's
    /// one `rand()` stream.
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
        let mut rain = std::mem::take(&mut self.casting.module_rain);
        rain.frame = rain.drops.step(
            rain.budget,
            DELTA_PER_TICK as u8,
            facing,
            rain.colour,
            &cam,
            || self.next_rand(),
        );
        self.spawn_rain_splashes();
        self.casting.module_rain = rain;
    }

    /// The splashes the rain routine's last call spawned: one
    /// `FUN_80021B04(drop, drop, 0x801F81F0, 0x1000)` each - the module's
    /// own record, whose colour halfwords `+0x22` / `+0x24` / `+0x26` the
    /// routine refreshes from the rain colour word on every call
    /// (`0x801F7C2C..0x801F7C3C`) - seated into the cast's running scene at
    /// the landed drop, with the drop's own trio as the angles (both pointer
    /// arguments are the record). The part's `+0x78`, which retail stores
    /// the halved depth into after the spawn, is not carried.
    fn spawn_rain_splashes(&mut self) {
        use legaia_overlay_images::summon_overlay::{SUMMON_OVERLAY_LINK_BASE, parse_records_at};
        let splashes = self.casting.module_rain.frame.splashes.clone();
        if splashes.is_empty() {
            return;
        }
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|p| p.module(legaia_engine_cast::cast_module_camera::SLIPPERY_ENTRY))
        else {
            return;
        };
        let off = (legaia_render_kernels::cast_rain::RAIN_SPLASH_RECORD - SUMMON_OVERLAY_LINK_BASE)
            as usize;
        let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
        offs.push(off);
        let mut bytes = module.bytes.to_vec();
        let c = self.casting.module_rain.colour;
        for (k, v) in [
            (0x22, (c >> 16) & 0xFF),
            (0x24, (c >> 8) & 0xFF),
            (0x26, c & 0xFF),
        ] {
            if let Some(b) = bytes.get_mut(off + k..off + k + 2) {
                b.copy_from_slice(&(v as u16).to_le_bytes());
            }
        }
        let parts: Vec<_> = parse_records_at(&bytes, &offs)
            .into_iter()
            .filter(|p| p.record_off == off)
            .collect();
        if parts.is_empty() {
            return;
        }
        let scene = self.casting.active_summon.get_or_insert_with(|| {
            legaia_engine_effects::summon::SummonScene::spawn_parts(
                &[],
                &bytes,
                crate::world::EFFECT_MODEL_LIBRARY_BASE,
                [0; 3],
            )
        });
        for s in splashes {
            scene.push_parts(&parts, &bytes, s.at, s.at);
        }
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
