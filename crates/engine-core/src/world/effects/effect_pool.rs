//! The effect-bundle pool: per-frame tick, the marker / sprite / model / ribbon
//! / kind-4 draw projections, and the spawn entry points.

use super::*;

/// Fixed lifetime (frames) of the dev-spawned [`DebugEffect`] entries -
/// engine-side visualization aids, not a retail cadence (retail effects
/// retire when their spawn records + child animations drain through
/// `Pool::tick_retail`).
pub const DEBUG_EFFECT_LIFETIME_FRAMES: u32 = 30;

/// Cap on simultaneous [`DebugEffect`] entries (mirrors the pool's 32
/// master slots so the debug exerciser degrades the same way).
pub const MAX_DEBUG_EFFECTS: usize = 32;

/// One live effect-pool child as a captured state holds it (pool
/// `_DAT_8007BD30 + 0x10 + slot * 0x20`): the pack0 batch it animates, its
/// frame cursor and 5.3 wait, its `rand() % 4` mirror bits and its 16.8
/// position and velocity. Installed by
/// [`World::install_effect_child_snapshot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EffectChildSeed {
    pub anim_id: u16,
    pub frame_count: u8,
    pub frame_cursor: u8,
    pub wait: u8,
    pub mirror: u8,
    pub pos: [i32; 3],
    pub velocity: [i16; 3],
}

impl World {
    /// Stand the effect pool's children where a captured state's stood
    /// (`Pool::install_child_snapshot`): the capture harness's seat for the
    /// 2D billboards, whose placement - a burning body's random pose object
    /// and jitter, a master's randomized legs, the mirror bits - is the
    /// `rand()` stream's history since the fight began.
    pub fn install_effect_child_snapshot(&mut self, seeds: &[EffectChildSeed]) {
        let children: Vec<legaia_engine_vm::effect_vm::ChildSlot> = seeds
            .iter()
            .map(|s| legaia_engine_vm::effect_vm::ChildSlot {
                frame_count: s.frame_count,
                mirror: s.mirror,
                frame_cursor: s.frame_cursor,
                wait: s.wait,
                velocity: s.velocity,
                pos: s.pos,
                anim_id: s.anim_id,
            })
            .collect();
        self.effect_pool.install_child_snapshot(&children);
    }

    /// Tick the effect pool - one sweep of the faithful retail walker
    /// (`Pool::tick_retail`, the `FUN_801E0088` pass-1 port): the master
    /// spawn cadence over the catalog's 14-byte pack1 records and the child
    /// anim/motion walk over the 6-byte pack0 frames, on the 5.3 fixed-point
    /// wait counters. One call = one retail logic frame (`frame_skip = 1`);
    /// [`Self::tick`] calls this on the retail-frame sub-clock so effect
    /// cadence tracks retail wall-speed.
    ///
    /// Also ages the engine-side [`Self::debug_effects`] (dev spawns kept
    /// outside the pool) over their fixed budget.
    ///
    /// REF: FUN_801E0088
    pub fn tick_effects(&mut self) {
        // The pool and the catalog leave the world for the walk, so the
        // host's `&mut World` aliases neither; `EffectHostImpl` reaches
        // only the RNG and the pending-burst queue.
        let mut pool = std::mem::take(&mut self.effect_pool);
        let catalog = std::mem::take(&mut self.effect_catalog);
        let mut host = EffectHostImpl { world: self };
        pool.tick_retail(&mut host, &catalog, 1);
        self.effect_pool = pool;
        self.effect_catalog = catalog;
        self.debug_effects.retain_mut(|d| {
            d.age_frames += 1;
            d.age_frames < DEBUG_EFFECT_LIFETIME_FRAMES
        });
    }

    /// Snapshot every live effect for the renderer: one [`EffectMarker`] per
    /// active master slot (`child_count > 0`) - i.e. per effect still in its
    /// spawn phase - plus one per live [`Self::debug_effects`] entry, with
    /// world position, spawn angle, and a coarse age fraction.
    ///
    /// This is the render-agnostic seam between the effect VM and the host's
    /// draw path. The host drains it each frame after [`Self::tick`] and emits
    /// whatever it can draw; nothing here depends on the renderer. The
    /// faithful per-child view is [`Self::active_effect_sprites`].
    pub fn active_effect_markers(&self) -> Vec<EffectMarker> {
        let lifetime = DEBUG_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        let mut out: Vec<EffectMarker> = self
            .effect_pool
            .master_slots
            .iter()
            .filter(|m| m.child_count > 0)
            .map(|m| EffectMarker {
                // Pool positions are 8.8 fixed-point world units.
                world_pos: [
                    (m.pos_x as f32) / 256.0,
                    (m.pos_y as f32) / 256.0,
                    (m.pos_z as f32) / 256.0,
                ],
                angle: m.angle,
                // `field_14` counts walker calls (a port-side aid); a master
                // only lives through its spawn phase, so normalize against
                // the nominal debug budget for a stable fade.
                age01: ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0),
            })
            .collect();
        out.extend(self.debug_effects.iter().map(|d| EffectMarker {
            world_pos: d.world_pos,
            angle: 0,
            age01: ((d.age_frames as f32) / lifetime).clamp(0.0, 1.0),
        }));
        out
    }

    /// Snapshot every live effect **child sprite** as a faithful billboard -
    /// the textured-quad seam that supersedes [`Self::active_effect_markers`]'
    /// one-cross-per-effect view. A direct mapping of the pool's live child
    /// slots through `Pool::child_billboards` (the `FUN_801E0088` pass-2
    /// port): each child's integrated 16.8 position, its current pack0
    /// frame's atlas rect + `tpage`/`clut`, the pass-2 sprite sizing, the
    /// retail brightness envelope, and the random UV-mirror corner order.
    ///
    /// Returns an empty vector when the catalog is empty (e.g. no disc) or
    /// no children are live, so it degrades cleanly.
    ///
    /// REF: FUN_801E0088
    pub fn active_effect_sprites(&self) -> Vec<EffectSprite> {
        self.effect_pool
            .child_billboards(&self.effect_catalog)
            .into_iter()
            .map(|b| EffectSprite {
                world_pos: [b.pos[0] as f32, b.pos[1] as f32, b.pos[2] as f32],
                size: [b.world_w.max(1) as f32, b.world_h.max(1) as f32],
                uv: [b.entry.u as u16, b.entry.v as u16],
                uv_size: [b.entry.w as u16, b.entry.h as u16],
                page: b.entry.page,
                clut: b.entry.clut,
                brightness: b.brightness,
                flip_h: b.flip_h,
                flip_v: b.flip_v,
                age01: (b.frame_cursor as f32 / b.frame_count.max(1) as f32).clamp(0.0, 1.0),
            })
            .collect()
    }

    /// Snapshot every live **debug** effect that has a 3D model assigned, for
    /// the `etmd`-model render path. One [`EffectModel`] per
    /// [`Self::debug_effects`] entry whose `model_index` is set. The host
    /// resolves `tmd_index` through [`Self::global_tmd`], builds a VRAM mesh,
    /// and draws it at `world_pos`.
    ///
    /// Distinct from [`Self::active_effect_sprites`] (the 2D billboard seam):
    /// effects like *Tail Fire* render as a small `etmd` mesh textured by the
    /// resident `etim` texels, not a billboard. The production effect-id ->
    /// etmd-model selection is driven by the move/art VM (the
    /// [`Self::spawn_move_fx`] / summon paths); this snapshot only carries the
    /// hand-spawned model exerciser.
    pub fn active_effect_models(&self) -> Vec<EffectModel> {
        let lifetime = DEBUG_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        self.debug_effects
            .iter()
            .filter_map(|d| {
                let tmd_index = d.model_index?;
                Some(EffectModel {
                    tmd_index,
                    world_pos: d.world_pos,
                    angle: 0,
                    age01: ((d.age_frames as f32) / lifetime).clamp(0.0, 1.0),
                })
            })
            .collect()
    }

    /// Dev/visualization helper: seat one synthetic effect carrying a 3D
    /// `etmd` model at `world_pos` (world units), so the model render path
    /// (e.g. *Tail Fire* = `etmd` mesh index 4, textured by `etim`) can be
    /// exercised by hand. `tmd_index` indexes [`Self::global_tmd_pool`]. The
    /// entry ages and retires through [`Self::tick_effects`]' fixed debug
    /// budget ([`DEBUG_EFFECT_LIFETIME_FRAMES`]).
    ///
    /// Like [`Self::spawn_debug_effect`], this is **not** a retail code path -
    /// it lives in [`Self::debug_effects`], outside the retail pool, so the
    /// faithful walker never sees it. Returns `false` when the debug list is
    /// at its cap.
    pub fn spawn_debug_effect_model(&mut self, world_pos: [f32; 3], tmd_index: usize) -> bool {
        if self.debug_effects.len() >= MAX_DEBUG_EFFECTS {
            return false;
        }
        self.debug_effects.push(DebugEffect {
            world_pos,
            model_index: Some(tmd_index),
            age_frames: 0,
        });
        true
    }

    /// This frame's effect ribbons - every draw-kind-4 ribbon node (move-VM op
    /// `0x42`) in the live summon, move-FX and effect-script scenes, rebuilt
    /// from its state ([`legaia_engine_effects::summon::SummonScene::ribbon_draws`]). Both
    /// battle hosts draw these in their FX pass, composed like a mesh part.
    ///
    /// Battle only: the emitter `FUN_801CFA48` is battle-overlay code reached
    /// by a direct `jal` from the SCUS render dispatcher (`0x8001B120`), so it
    /// exists only while PROT 0898 holds slot A.
    ///
    /// PORT: FUN_8001ADA4 (case 4's `0x2000` arm, `0x8001B060..0x8001B124`:
    /// the emitter call and the model-list repoint)
    pub fn active_effect_ribbons(&self) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        if self.mode != legaia_engine_mode::mode::SceneMode::Battle {
            return Vec::new();
        }
        let mut out = Vec::new();
        let scenes = self
            .casting
            .active_summon
            .iter()
            .chain(self.casting.active_move_fx.iter())
            .chain(self.casting.active_action_fx.iter());
        for scene in scenes {
            out.extend(scene.ribbon_draws());
        }
        out
    }

    /// This frame's draw-kind-4 geometry - every live node whose model is
    /// its emitter's per-frame output, each composed like a mesh part: the
    /// ribbons ([`Self::active_effect_ribbons`], battle only), the `0x4000`
    /// sprite-arm quads ([`legaia_engine_effects::effect_sprite_arm`]) and the default arm's
    /// procedural rings / discs / crowns ([`legaia_engine_effects::effect_default_arm`]) of
    /// the summon, move-FX, effect-script and field ambient parts. This is
    /// the list both hosts' FX passes draw.
    pub fn active_effect_kind4_draws(
        &self,
    ) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        let mut out = self.active_effect_ribbons();
        let scenes = self
            .casting
            .active_summon
            .iter()
            .chain(self.casting.active_move_fx.iter())
            .chain(self.casting.active_action_fx.iter())
            .chain(self.casting.mystic_shield_fx.iter());
        for scene in scenes {
            out.extend(scene.sprite_arm_draws());
            out.extend(scene.default_arm_draws());
        }
        out.extend(self.ambient_sprite_arm_draws());
        out.extend(self.ambient_default_arm_draws());
        // The fishing venue's ambient ripples: default-arm rings too.
        out.extend(self.minigames.fishing_venue.ripple_draws());
        out
    }

    /// Dev/visualization helper: seat one synthetic marker effect at
    /// `world_pos` (world units) so the effect render bridge can be exercised
    /// by hand. It ages and retires through [`Self::tick_effects`]' fixed
    /// debug budget ([`DEBUG_EFFECT_LIFETIME_FRAMES`]).
    ///
    /// This is **not** a retail code path - it lives in
    /// [`Self::debug_effects`], outside the retail pool, so the faithful
    /// walker never consumes it as a spawn script. The real catalog (PROT
    /// 0873 `efect.dat`) loads at scene entry, so `ui_element` spawns resolve
    /// to real scripts; use [`Self::try_spawn_effect`] for the production
    /// path. Returns `false` when the debug list is at its cap.
    pub fn spawn_debug_effect(&mut self, world_pos: [f32; 3]) -> bool {
        if self.debug_effects.len() >= MAX_DEBUG_EFFECTS {
            return false;
        }
        self.debug_effects.push(DebugEffect {
            world_pos,
            model_index: None,
            age_frames: 0,
        });
        true
    }

    /// Spawn effect `ui_id` at `world_pos` / `angle` via the pool, looking
    /// up the script in `self.effect_catalog`. No-op when the catalog is
    /// empty or the id is out of range. Mirrors the retail pool spawner
    /// `FUN_801DFDF0` as the effect-script walk `FUN_801DEA50` reaches it
    /// ([`Self::route_battle_effect_spawns`]).
    pub fn try_spawn_effect(&mut self, ui_id: u8, world_pos: [i16; 3], angle: u16) {
        self.spawn_effect_caught_up(ui_id, world_pos, angle, 0);
    }

    /// [`Self::try_spawn_effect`] for a spawn retail makes **ahead of** the
    /// frame's walker that the engine seats after it: the new master then
    /// takes the `sweeps` this frame's walker gave every other
    /// ([`legaia_engine_vm::effect_vm::Pool::catch_up_master`]).
    pub(crate) fn spawn_effect_caught_up(
        &mut self,
        ui_id: u8,
        world_pos: [i16; 3],
        angle: u16,
        sweeps: u8,
    ) {
        // Same hand-off as [`Self::tick_effects`].
        let mut pool = std::mem::take(&mut self.effect_pool);
        let catalog = std::mem::take(&mut self.effect_catalog);
        let mut host = EffectHostImpl { world: self };
        if let Some(mi) = pool.spawn_by_ui_id(&mut host, ui_id, world_pos, angle, &catalog) {
            pool.catch_up_master(&mut host, &catalog, mi, sweeps);
        }
        self.effect_pool = pool;
        self.effect_catalog = catalog;
    }
}
