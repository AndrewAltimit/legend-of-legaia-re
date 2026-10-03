//! The volumetric ground-fog enhancement's `World` seam: which scene raises
//! a bank, who parts it, and the frame both play hosts draw.
//!
//! Nothing here is retail - see [`crate::fog_volume`]. The population that
//! parts the bank is the one that casts a drop shadow
//! ([`World::field_drop_shadows`]): the player and every placed field
//! channel with a position, plus every active body in battle.

use super::*;
use crate::fog_volume::{FogMover, FogSpace, FogVolumeFrame, scene_style};

/// Mover keys: the player, field channels by placement slot, battle bodies
/// by actor index - disjoint ranges so a mode switch never pairs one with
/// another's history.
const KEY_PLAYER: u32 = 0;
const KEY_CHANNEL: u32 = 0x100;
const KEY_BATTLE: u32 = 0x1000;

impl World {
    /// The bank the current state asks for: the field scene's own style
    /// ([`crate::fog_volume::scene_style`] - its tuned entry, else the retail
    /// fog pool's gate with an enabled region); in battle, the style of the
    /// field scene the fight was entered from. `None` elsewhere.
    pub fn fog_volume_style(&self) -> Option<crate::fog_volume::FogStyle> {
        match self.mode {
            SceneMode::Field => {
                let pool_live = self.fog.gate && self.fog.regions.iter().any(|r| r.enabled);
                scene_style(&self.active_scene_label, pool_live)
            }
            SceneMode::Battle => self.fog_volume.last_style,
            _ => None,
        }
    }

    /// The actors that part the bank this tick, in the bank's space.
    pub fn fog_volume_movers(&self) -> Vec<FogMover> {
        let mut out = Vec::new();
        match self.mode {
            SceneMode::Field => {
                if let Some(a) = self
                    .player_actor_slot
                    .and_then(|s| self.actors.get(usize::from(s)))
                {
                    out.push(FogMover {
                        key: KEY_PLAYER,
                        x: f32::from(a.move_state.world_x),
                        z: f32::from(a.move_state.world_z),
                    });
                }
                let hide = FIELD_OFFMAP_HIDE_XZ;
                for c in self.field_vm.channels.iter().filter(|c| !c.object_bind) {
                    if c.ctx.move_id == 0 {
                        continue;
                    }
                    let Ok(slot) = u8::try_from(c.placement_index) else {
                        continue;
                    };
                    let (x, z) = self
                        .npcs
                        .positions
                        .get(&slot)
                        .copied()
                        .unwrap_or((c.ctx.world_x as i16, c.ctx.world_z as i16));
                    if x == hide && z == hide {
                        continue;
                    }
                    let key = KEY_CHANNEL + u32::from(slot);
                    // One mover per placement, however many channels it runs.
                    if out.iter().any(|m: &FogMover| m.key == key) {
                        continue;
                    }
                    out.push(FogMover {
                        key,
                        x: f32::from(x),
                        z: f32::from(z),
                    });
                }
            }
            SceneMode::Battle => {
                for (i, a) in self.actors.iter().enumerate().filter(|(_, a)| a.active) {
                    out.push(FogMover {
                        key: KEY_BATTLE + i as u32,
                        x: f32::from(a.move_state.world_x),
                        z: f32::from(a.move_state.world_z),
                    });
                }
            }
            _ => {}
        }
        out
    }

    /// One tick of the bank (called from [`World::tick`]). Off resets it,
    /// so a host with the toggle down draws nothing and keeps no state.
    pub fn tick_fog_volume(&mut self) {
        if !self.toggles.volumetric_fog {
            if self.fog_volume.last_style.is_some() || self.fog_volume.strength > 0.0 {
                self.fog_volume.reset();
            }
            return;
        }
        // A new field scene starts its own bank (a battle keeps the label of
        // the scene it was entered from, and inherits that bank).
        if self.mode == SceneMode::Field && self.fog_volume.scene != self.active_scene_label {
            self.fog_volume.scene.clone_from(&self.active_scene_label);
            self.fog_volume.last_style = None;
            self.fog_volume.strength = 0.0;
            self.fog_volume.clear_field();
        }
        let (space, focus) = match self.mode {
            SceneMode::Field => {
                let p = self.fog_player_world_pos();
                (FogSpace::Field, [p[0] as f32, p[2] as f32])
            }
            SceneMode::Battle => (FogSpace::Battle, [0.0, 0.0]),
            _ => {
                // Menus, the world map, cutscene modes: hold the bank as it
                // is; the frame accessor draws nothing outside field/battle.
                return;
            }
        };
        let style = self.fog_volume_style();
        self.fog_volume.set_space(space);
        self.fog_volume.set_style(style);
        let movers = self.fog_volume_movers();
        // Borrow split: the floor sampler reads terrain, the step writes the
        // bank.
        let mut bank = std::mem::take(&mut self.fog_volume);
        match space {
            FogSpace::Field => bank.step(focus, &movers, |x, z| {
                self.sample_field_floor_height(x as i32, z as i32) as f32
            }),
            FogSpace::Battle => bank.step(focus, &movers, |_, _| 0.0),
        }
        self.fog_volume = bank;
    }

    /// This frame's bank for a host to draw, or `None` - the toggle down, a
    /// mode with no bank (anything but a field scene or a battle), or no
    /// style raised. The scripted screen tint
    /// ([`World::scene_screen_tint`]) is folded into the colour so a fade to
    /// black takes the fog with it.
    pub fn fog_volume_frame(&self) -> Option<FogVolumeFrame<'_>> {
        if !self.toggles.volumetric_fog
            || !matches!(self.mode, SceneMode::Field | SceneMode::Battle)
        {
            return None;
        }
        let want = match self.mode {
            SceneMode::Field => FogSpace::Field,
            _ => FogSpace::Battle,
        };
        if self.fog_volume.space != want {
            return None;
        }
        self.fog_volume.frame(self.scene_screen_tint())
    }
}
