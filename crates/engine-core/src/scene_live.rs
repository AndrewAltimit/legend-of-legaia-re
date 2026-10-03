//! **Live scene preview**: a field scene entered through the real
//! [`SceneHost`] and ticked headless (no pad), so a viewer animates exactly
//! what the play hosts animate on the same scene.
//!
//! A static full-map view ([`crate::scene_assembly`]) bakes the draw lists
//! once, against the floor-height ladder the scene's MAN header ships. The
//! running game moves several things the bake cannot see, all of them driven
//! by the scene's own scripts on the world tick:
//!
//! - the **floor-height ladder** (field-VM op `0x4C` nibble 9): `jouina`'s
//!   pulsing path, `concnow`'s flesh pits - and `concnow`'s entry script
//!   replaces the whole ladder (`4C 9E`), so its baked heights are not the
//!   heights the scene is ever shown at;
//! - **placed-prop clips** (the prop bank's cursors, retail `FUN_800204F8`):
//!   the Rim Elm windmill's sails;
//! - the **ambient move-VM tree** and the scripted VRAM effects (palette
//!   cyclers, lightning, VDF vertex morphs).
//!
//! This type owns no second implementation of any of them: it is a
//! [`SceneHost`] entered the way the play pages' scene picker enters it
//! ([`crate::world::World::stage_picker_entry`] then
//! [`SceneHost::enter_field_scene`]), stepped by [`SceneHost::tick`], and
//! read through the same kernels the play hosts call -
//! [`FloorWave`] for the placed / terrain draws,
//! [`crate::field_ground::live_render_positions`] for the walk ground,
//! [`crate::field_env::PropAnimBank::pose_key`] for props and
//! [`crate::world::World::step_field_vram_effects`] for VRAM.
//!
//! A viewer never follows the scene out: a door, warp or scripted battle the
//! headless world reaches re-enters the viewed scene instead (its entry
//! script runs again, as on a fresh visit), and after
//! [`MAX_RESTARTS`] such re-entries the preview stops ticking and the view
//! holds its last frame.
//!
//! Overworld scenes are not previewed live: their ground does not follow the
//! field ladder and their animation is the CLUT walker, which a viewer drives
//! on its own ([`crate::clut_walk_anim`]).

use std::sync::Arc;

use crate::field_env::{EnvDraw, FloorWave, PropPoseKey};
use crate::scene::{ProtIndex, SceneHost, SceneTickEvent};
use crate::world::SceneMode;

/// Re-entries of the viewed scene a preview performs before it stops
/// ticking (see the module docs).
pub const MAX_RESTARTS: u32 = 3;

/// A headless, live field scene (see the module docs).
pub struct LiveScene {
    /// The engine scene host the preview runs. Public so a viewer can read
    /// any further world state through the host's own accessors.
    pub host: SceneHost,
    name: String,
    /// The ladder the scene's MAN header ships (MAN frame) - the one a static
    /// assembly resolved every draw against, and the base of [`FloorWave`].
    scene_lut: Option<[i16; 16]>,
    restarts: u32,
    halted: bool,
}

impl LiveScene {
    /// Enter `name` (a CDNAME field scene) as a scene-picker visit. `Err`
    /// for an overworld scene or when the host refuses the scene.
    pub fn enter(index: Arc<ProtIndex>, name: &str) -> Result<Self, String> {
        if crate::scene::is_world_map_scene(name) {
            return Err(format!("{name}: overworld scenes are not previewed live"));
        }
        let mut host = SceneHost::new(index);
        Self::enter_host(&mut host, name)?;
        let scene_lut = host
            .scene
            .as_ref()
            .and_then(|s| s.field_floor_height_lut(&host.index).ok().flatten());
        Ok(Self {
            host,
            name: name.to_string(),
            scene_lut,
            restarts: 0,
            halted: false,
        })
    }

    fn enter_host(host: &mut SceneHost, name: &str) -> Result<(), String> {
        // The scene picker's free-roam staging, the one rule both play hosts
        // apply to a picker entry.
        host.world.stage_picker_entry(name, false);
        host.world.npcs.animate = true;
        host.enter_field_scene(name, 0)
            .map_err(|e| format!("enter {name}: {e:#}"))
    }

    /// The scene this preview shows.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the preview still ticks.
    pub fn is_live(&self) -> bool {
        !self.halted
    }

    /// How many times the headless world left the scene and was re-entered.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// Advance one retail vsync (one [`SceneHost::tick`]). Returns `false`
    /// once the preview has stopped ticking.
    pub fn tick(&mut self) -> bool {
        if self.halted {
            return false;
        }
        let event = self.host.tick();
        let left = match &event {
            Err(_) => true,
            Ok(SceneTickEvent::SceneEntered { .. }) => true,
            Ok(_) => {
                self.host.world.mode != SceneMode::Field
                    || self.host.scene.as_ref().map(|s| s.name.as_str()) != Some(self.name.as_str())
            }
        };
        if left {
            self.restarts += 1;
            let name = self.name.clone();
            if self.restarts > MAX_RESTARTS || Self::enter_host(&mut self.host, &name).is_err() {
                self.halted = true;
                return false;
            }
        }
        true
    }

    /// The ladder the scene's MAN header ships (MAN frame).
    pub fn scene_floor_lut(&self) -> Option<[i16; 16]> {
        self.scene_lut
    }

    /// The live floor-height ladder (scratchpad frame, `0x1F80035C`).
    pub fn live_floor_lut(&self) -> [i16; 16] {
        self.host.world.terrain.floor_height_lut
    }

    /// The wave between the shipped ladder and the live one; `None` while
    /// they agree.
    pub fn floor_wave(&self) -> Option<FloorWave> {
        FloorWave::from_scene_and_world(self.scene_lut, &self.host.world.terrain.floor_height_lut)
    }

    /// Per-draw Y offsets (retail frame) of `draws` - resolved against the
    /// shipped ladder - under the live one. `None` while the ladder sits
    /// where the scene shipped it.
    pub fn floor_wave_offsets(&self, draws: &[EnvDraw]) -> Option<Vec<i32>> {
        self.floor_wave().map(|w| w.offsets(draws))
    }

    /// The live pose key of a placed draw whose bind names a clip (`None` for
    /// a static prop or one the prop bank does not drive).
    pub fn prop_pose_key(&self, draw: &EnvDraw) -> Option<PropPoseKey> {
        if draw.anim_id == 0 {
            return None;
        }
        self.host.world.props.bank.pose_key(draw.anchor)
    }
}
