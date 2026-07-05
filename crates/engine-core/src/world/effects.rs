//! Move bytecode/system flags/field-script loading, actor spawning, effect markers/sprites/models, summons, field stagers, and move FX.
//!
//! Split out of `world.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// Set / clear the move-VM bytecode for `slot`. `None` clears the
    /// buffer; subsequent ticks won't run the move VM on this actor.
    pub fn set_move_bytecode(&mut self, slot: usize, bytecode: Option<Vec<u16>>) {
        if slot < self.move_bytecode.len() {
            self.move_bytecode[slot] = bytecode.unwrap_or_default();
        }
    }

    /// Set bit `idx` in the shared system flag bank. `idx >> 3` is the byte
    /// offset; the bit mask is `0x80 >> (idx & 7)` (MSB-first, mirroring the
    /// SCUS helper at `FUN_8003CE08`). The bank grows lazily as needed.
    pub fn system_flag_set(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte >= self.system_flags.len() {
            self.system_flags.resize(byte + 1, 0);
        }
        self.system_flags[byte] |= 0x80u8 >> (idx & 7);
    }

    /// Clear bit `idx` in the shared system flag bank. See [`system_flag_set`].
    /// Out-of-bounds clears are no-ops (the bit is already zero).
    ///
    /// [`system_flag_set`]: World::system_flag_set
    pub fn system_flag_clear(&mut self, idx: u16) {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] &= !(0x80u8 >> (idx & 7));
        }
    }

    /// Test bit `idx` in the shared system flag bank. Returns `false` for
    /// indices past the currently-grown size.
    pub fn system_flag_test(&self, idx: u16) -> bool {
        let byte = (idx >> 3) as usize;
        if byte < self.system_flags.len() {
            self.system_flags[byte] & (0x80u8 >> (idx & 7)) != 0
        } else {
            false
        }
    }

    /// Replace the field-VM bytecode buffer + reset PC. Engines call this
    /// when entering a new field scene (loading the scene's per-event
    /// script) to start interpretation from the beginning.
    pub fn load_field_script(&mut self, bytecode: Vec<u8>) {
        self.field_bytecode = bytecode;
        self.field_pc = 0;
        self.field_ctx = FieldCtx::default();
    }

    /// Load a field-VM bytecode buffer and begin interpretation at `pc`
    /// instead of 0.
    ///
    /// Used to run a MAN-resolved **scene-entry system script** (retail
    /// `FUN_8003ab2c`, context channel `0xFB`): the buffer is the MAN slice
    /// taken from the script block's start, and `pc` is the first opcode's
    /// offset into that slice (past the `[local-count][locals][record-header]`
    /// prefix). Slicing from the script start keeps relative jumps wrapping
    /// against the slice base (index 0), matching the retail
    /// `buffer_base = script_start` convention. See
    /// [`crate::scene::Scene::field_man_entry_script`].
    ///
    /// REF: FUN_8003ab2c (the port lives in `legaia_asset::man_section`).
    pub fn load_field_script_at(&mut self, bytecode: Vec<u8>, pc: usize) {
        self.field_bytecode = bytecode;
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
    }

    /// Load one event-script record into the field VM, skipping the leading
    /// `0xFFFF 0x0000` frame-divider sentinel when present.
    ///
    /// Records pulled from `scene_event_scripts` / `scene_scripted_asset_table`
    /// containers commonly open with the 4-byte sentinel; the field VM's
    /// dispatcher in retail consumes the sentinel as a record-start marker
    /// rather than an opcode (the high bit + low-7-bits 0x7F would otherwise
    /// hit the "UNFIND INDICATION" default arm). The exact dispatcher prelude
    /// hasn't been fully traced, so this skip is heuristic - revise once
    /// `FUN_801DE840`'s outer loop is captured.
    pub fn load_field_record(&mut self, record_bytes: &[u8]) {
        const FRAME_DIVIDER: [u8; 4] = [0xFF, 0xFF, 0x00, 0x00];
        let pc = if record_bytes.starts_with(&FRAME_DIVIDER) {
            4
        } else {
            0
        };
        self.field_bytecode = record_bytes.to_vec();
        self.field_pc = pc;
        self.field_ctx = FieldCtx::default();
    }

    /// Activate a slot and return a mutable reference to the actor.
    ///
    /// PORT: FUN_80020DE0
    pub fn spawn_actor(&mut self, slot: usize) -> &mut Actor {
        let a = &mut self.actors[slot];
        a.active = true;
        a
    }

    /// Ensure the slot at `id` is initialized with the supplied default
    /// position and active. Idempotent.
    ///
    /// Preserves `tmd_binding` and `active_animation` across the reset so
    /// that `init_scene_animations` bindings survive the first field-VM
    /// actor-spawn opcode.
    pub fn ensure_actor(&mut self, id: u8, default_pos: ActorVmPosition) -> &mut Actor {
        let a = &mut self.actors[id as usize];
        if !a.active {
            let tmd_binding = a.tmd_binding;
            let active_animation = a.active_animation.take();
            *a = Actor::new();
            a.tmd_binding = tmd_binding;
            a.active_animation = active_animation;
            a.active = true;
        }
        a.default_pos = default_pos;
        a
    }

    /// Pre-bind every actor slot to its scene resources before the field VM
    /// spawns actors. Wires:
    ///
    /// - `actor.tmd_binding = slot_idx` (direct 1:1 ordering: the retail
    ///   `FUN_8001E890` loop registers TMDs in pack offset-table order -
    ///   actor K → TMD slot K).
    /// - `actor.active_animation` seeded from ANM record 0 (idle) when an
    ///   ANM pack is present for that slot.
    ///
    /// Because `ensure_actor` preserves these fields across resets, the
    /// bindings survive the first field-VM actor-spawn opcode.
    pub fn init_scene_animations(&mut self, resources: &crate::scene_resources::SceneResources) {
        for (i, actor) in self.actors.iter_mut().enumerate() {
            if i < resources.tmds.len() {
                actor.tmd_binding = Some(i);
            }
            if actor.active_animation.is_none()
                && let Some(anm) = resources.anm_pack_for_actor(i)
                && let Some(rec_bytes) = anm.record_bytes(0)
            {
                let bone_count = resources
                    .tmds
                    .get(i)
                    .map(|t| t.tmd.objects.len())
                    .unwrap_or(1)
                    .max(1);
                if let Ok(player) = AnimPlayer::new(rec_bytes.to_vec(), bone_count) {
                    actor.active_animation = Some(player);
                }
            }
        }
    }

    /// Run the actor VM bytecode against this world.
    ///
    /// Convenience wrapper around [`vm::run`] that constructs a host borrow.
    pub fn run_actor_bytecode(&mut self, bytecode: &[u8]) -> Result<usize, vm::VmError> {
        let mut host = ActorVmHostImpl { world: self };
        vm::run(&mut host, bytecode)
    }

    /// Step the move VM once for the actor at `slot`, using `bytecode` as
    /// the move buffer. Returns the [`vm::move_vm::StepResult`].
    ///
    /// Engines typically call this in a loop on each per-frame actor tick
    /// until the inner step returns `Halt` or `Wait`.
    ///
    /// Writes the host's `move_bytecode_write_u16` calls (issued by ext
    /// sub-ops 0x04 / 0x1B / 0x1E / 0x36) back to `world.move_bytecode[slot]`
    /// after step completes - see the `MoveVmHostImpl` deferred-writes map.
    pub fn step_move_vm(&mut self, slot: usize, bytecode: &[u16]) -> vm::move_vm::StepResult {
        let mut host = MoveVmHostImpl {
            world: self,
            current_slot: Some(slot),
            deferred_writes: std::collections::BTreeMap::new(),
        };
        let actor_state = unsafe {
            // SAFETY: the host borrows `world.actors[slot]` only through
            // queries that don't read this slot's `move_state`. The host
            // implementation never touches `actors[slot].move_state`; it
            // only reads sin/cos LUTs and other engine-side data.
            &mut *(&mut host.world.actors[slot].move_state as *mut MoveActorState)
        };
        let result = vm::move_vm::step(&mut host, actor_state, bytecode);
        let writes = std::mem::take(&mut host.deferred_writes);
        if !writes.is_empty()
            && let Some(buf) = self.move_bytecode.get_mut(slot)
        {
            for (off, value) in writes {
                if off >= buf.len() {
                    buf.resize(off + 1, 0);
                }
                buf[off] = value;
            }
        }
        result
    }

    /// Run one battle-action state-machine step.
    pub fn step_battle(&mut self) -> StepOutcome {
        // Anim commit first (the retail frame order: FUN_8004AD80 ran last
        // frame after the SM staged its id), so the SM observes converged
        // `current_anim` / cleared `ADVANCE_DONE` for clip-less actors and
        // sees in-flight staged clips otherwise.
        self.commit_staged_battle_anims();
        let ctx_ptr: *mut BattleActionCtx = &mut self.battle_ctx;
        let mut host = BattleHostImpl { world: self };
        // SAFETY: BattleHostImpl never reads or writes `world.battle_ctx`
        // through the borrow; it only touches `actors`, helper tables, and
        // call-records.
        let ctx = unsafe { &mut *ctx_ptr };
        vm::battle_action::step(&mut host, ctx)
    }

    /// Tick the effect pool.
    pub fn tick_effects(&mut self) {
        let pool_ptr: *mut Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl never reads `world.effect_pool` through
        // the borrow.
        let pool = unsafe { &mut *pool_ptr };
        pool.tick(&mut host);
    }

    /// Snapshot every live effect for the renderer: one [`EffectMarker`] per
    /// active master slot (`child_count > 0`), with its world position, spawn
    /// angle, and lifetime fraction.
    ///
    /// This is the render-agnostic seam between the effect VM and the host's
    /// draw path. The host drains it each frame after [`Self::tick`] and emits
    /// whatever it can draw; nothing here depends on the renderer.
    pub fn active_effect_markers(&self) -> Vec<EffectMarker> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        self.effect_pool
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
                age01: ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0),
            })
            .collect()
    }

    /// Snapshot every live effect's **child sprites** as faithful billboards -
    /// the textured-quad seam that supersedes [`Self::active_effect_markers`]'
    /// one-cross-per-effect view. For each active master slot it resolves the
    /// effect's children through the loaded [`crate::world::World::effect_catalog`],
    /// walks each child's pack0 animation to the current frame, and reads the
    /// frame's sprite-atlas entry for size + VRAM `(u, v)` / `tpage` / `clut`.
    ///
    /// Mirrors the retail per-frame walker (`FUN_801E0088` pass 2): one GPU
    /// sprite primitive per child, sized from the atlas, placed at the effect
    /// origin plus the child's spread offset. Returns an empty vector when the
    /// catalog is empty (e.g. no disc), so it degrades cleanly.
    pub fn active_effect_sprites(&self) -> Vec<EffectSprite> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        let atlas = self.effect_catalog.atlas();
        let mut out = Vec::new();
        for m in self.effect_pool.master_slots.iter() {
            if m.child_count == 0 {
                continue;
            }
            let Some((_script, children)) = self.effect_catalog.entry(m.ui_id) else {
                continue;
            };
            let age01 = ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0);
            let frame_idx = m.field_14.max(0) as usize;
            let origin = [
                m.pos_x as f32 / 256.0,
                m.pos_y as f32 / 256.0,
                m.pos_z as f32 / 256.0,
            ];
            for (i, child) in children.iter().enumerate() {
                // Resolve the current animation frame -> atlas entry.
                let entry = self.effect_catalog.anim(child.sprite_id).and_then(|batch| {
                    if batch.frames.is_empty() {
                        return None;
                    }
                    // Loop the batch over the effect lifetime (the faithful
                    // per-frame token cadence is not extracted; a uniform loop
                    // keeps the sprite animating over its visible life).
                    let f = frame_idx % batch.frames.len();
                    atlas.get(batch.frames[f].atlas_index as usize)
                });
                let Some(e) = entry else {
                    continue;
                };
                // Child placement: the stored spread offset (random-distribution
                // path) or a small deterministic ring (walker-populated path).
                let (dx, dz) = m
                    .child_offsets
                    .get(i)
                    .copied()
                    .map(|(x, z)| (x as f32 / 256.0, z as f32 / 256.0))
                    .unwrap_or_else(|| {
                        let a = i as f32 * std::f32::consts::TAU / children.len().max(1) as f32;
                        let r = (child.width.max(child.depth).max(8) as f32) / 256.0;
                        (a.cos() * r, a.sin() * r)
                    });
                out.push(EffectSprite {
                    world_pos: [origin[0] + dx, origin[1], origin[2] + dz],
                    size: [e.w.max(1) as f32, e.h.max(1) as f32],
                    uv: [e.u as u16, e.v as u16],
                    uv_size: [e.w as u16, e.h as u16],
                    page: e.page,
                    clut: e.clut,
                    age01,
                });
            }
        }
        out
    }

    /// Snapshot every live effect that has a 3D model assigned, for the
    /// `etmd`-model render path. One [`EffectModel`] per active master slot
    /// whose `model_index` is set (the model-driven effects); 2D-billboard-only
    /// effects are skipped. The host resolves `tmd_index` through
    /// [`Self::global_tmd`], builds a VRAM mesh, and draws it at `world_pos`.
    ///
    /// Distinct from [`Self::active_effect_sprites`] (the 2D billboard seam):
    /// effects like *Tail Fire* render as a small `etmd` mesh textured by the
    /// resident `etim` texels, not a billboard.
    pub fn active_effect_models(&self) -> Vec<EffectModel> {
        let lifetime = vm::effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES.max(1) as f32;
        self.effect_pool
            .master_slots
            .iter()
            .filter(|m| m.child_count > 0)
            .filter_map(|m| {
                let tmd_index = m.model_index?;
                Some(EffectModel {
                    tmd_index,
                    world_pos: [
                        m.pos_x as f32 / 256.0,
                        m.pos_y as f32 / 256.0,
                        m.pos_z as f32 / 256.0,
                    ],
                    angle: m.angle,
                    age01: ((m.field_14 as f32) / lifetime).clamp(0.0, 1.0),
                })
            })
            .collect()
    }

    /// Snapshot every placed overworld entity for the renderer: one
    /// [`WorldMapEntityMarker`] per installed entity that carries a world
    /// position, paired with its coarse [`WorldMapEntityKind`].
    ///
    /// Returns an empty vector unless the disc-placement seeding
    /// ([`Self::install_world_map_entities_at`]) populated
    /// [`Self::world_map_entity_positions`] - the config-only installers
    /// (which leave positions empty) produce no markers, so a camera-only or
    /// synthetic world map degrades cleanly. The marker `y` is the player
    /// actor's current plane (the placements are 2D), so markers sit on the
    /// player's walking plane rather than at an arbitrary `y = 0`.
    pub fn world_map_entity_markers(&self) -> Vec<WorldMapEntityMarker> {
        if self.world_map_entity_positions.is_empty() {
            return Vec::new();
        }
        let base_y = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| a.move_state.world_y as f32)
            .unwrap_or(0.0);
        self.world_map_entity_positions
            .iter()
            .enumerate()
            .map(|(i, &(x, z))| {
                let kind = match self.world_map_entity_configs.get(i) {
                    Some(WorldMapEntityConfig::EncounterZone { .. }) => {
                        WorldMapEntityKind::EncounterZone
                    }
                    Some(WorldMapEntityConfig::Portal { .. })
                    | Some(WorldMapEntityConfig::OverworldPortal { .. }) => {
                        WorldMapEntityKind::Portal
                    }
                    // An NPC config or no config at all (a plain interaction)
                    // both render as the NPC marker.
                    Some(WorldMapEntityConfig::Npc { .. }) | None => WorldMapEntityKind::Npc,
                };
                WorldMapEntityMarker {
                    world_pos: [x as f32, base_y, z as f32],
                    kind,
                }
            })
            .collect()
    }

    /// The player's overworld position for the renderer, or `None` when there
    /// is no active player actor. The world-map draw path shows the player at
    /// this position (the player's own mesh isn't drawn in
    /// [`SceneMode::WorldMap`]), oriented by [`WorldMapPlayerMarker::facing`].
    pub fn world_map_player_marker(&self) -> Option<WorldMapPlayerMarker> {
        let slot = self.player_actor_slot? as usize;
        let a = self.actors.get(slot)?;
        if !a.active {
            return None;
        }
        Some(WorldMapPlayerMarker {
            world_pos: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            facing: a.move_state.render_26,
        })
    }

    /// Dev/visualization helper: seat one synthetic active effect carrying a
    /// 3D `etmd` model at `world_pos` (world units), so the model render path
    /// (e.g. *Tail Fire* = `etmd` mesh index 4, textured by `etim`) can be
    /// exercised by hand. `tmd_index` indexes [`Self::global_tmd_pool`]. The
    /// slot ages and retires through the normal [`Self::tick_effects`] lifetime.
    ///
    /// Like [`Self::spawn_debug_effect`], this is **not** a retail code path -
    /// the production effect-id -> etmd-model selection (driven by the move/art
    /// VM) is not yet decoded. Returns `false` when the pool is full.
    pub fn spawn_debug_effect_model(&mut self, world_pos: [f32; 3], tmd_index: usize) -> bool {
        let Some(slot) = self.effect_pool.allocate_master() else {
            return false;
        };
        let m = &mut self.effect_pool.master_slots[slot];
        *m = vm::effect_vm::MasterSlot::default();
        m.child_count = 1;
        m.pos_x = (world_pos[0] * 256.0) as i32;
        m.pos_y = (world_pos[1] * 256.0) as i32;
        m.pos_z = (world_pos[2] * 256.0) as i32;
        m.model_index = Some(tmd_index);
        true
    }

    /// Spawn a Seru-magic summon scene-graph from a parsed stager overlay (e.g.
    /// extraction PROT 0903, Gimard *Burning Attack*) at `origin` (world units).
    /// `record_bytes` is the overlay's raw bytes (the buffer `overlay` was parsed
    /// from); `model_base` is the pool index a part's `model_sel == 0` resolves to
    /// (the summon's mesh-set base, e.g. [`crate::scene::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    /// Replaces any in-flight summon. Tick it with [`Self::tick_summon`].
    ///
    /// NOTE this drives the engine's **move-VM scene-graph stand-in**
    /// ([`crate::summon::SummonScene`]), not the faithful player-summon render. A
    /// live trace resolved that retail draws the player summon as an ordinary
    /// battle actor via the per-object TRS-keyframe path `FUN_80048A08` /
    /// `FUN_8004998C` (ported in [`legaia_engine_vm::anim_vm`]); see the
    /// `SummonScene` module reconciliation note.
    // REF: FUN_80048A08 (faithful player-summon render = battle-actor TRS-keyframe draw)
    pub fn spawn_summon(
        &mut self,
        overlay: &legaia_asset::summon_overlay::SummonOverlay,
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) {
        self.active_summon = Some(crate::summon::SummonScene::spawn(
            overlay,
            record_bytes,
            model_base,
            origin,
        ));
    }

    /// Advance the active summon one frame through the move VM. No-op when no
    /// summon is playing; drains the scene once every part has finished.
    /// `frame_delta` is the per-part wait-timer drain (anim-speed × frame-rate).
    pub fn tick_summon(&mut self, frame_delta: u16) {
        let Some(mut scene) = self.active_summon.take() else {
            return;
        };
        {
            // Borrow split: the move-VM host borrows the rest of `World` (sin
            // LUT etc.) while the scene's part states live in `scene`, taken out
            // above. `current_slot = None` - summon parts are not World actors,
            // so the slot-routed callbacks are inert for them.
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
            };
            scene.tick(&mut host, frame_delta);
        }
        if !scene.finished() {
            self.active_summon = Some(scene);
        }
    }

    /// Per-part render draws for the active summon's mesh-bearing parts (empty
    /// when no summon is playing). Each draw's `model_index` indexes
    /// [`Self::global_tmd_pool`]. See [`crate::summon::SummonScene::part_draws`]
    /// for the faithful-tick / interpreted-transform boundary.
    pub fn active_summon_part_draws(&self) -> Vec<crate::summon::SummonPartDraw> {
        self.active_summon
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default()
    }

    /// Install the current scene's field move-VM stager table from the scene's
    /// event-scripts container bytes (a `SceneEventScripts` /
    /// `SceneScriptedAssetTable` entry). Parses the prescript records as
    /// summon-format move-VM stagers and retains both the parsed records and the
    /// bundle bytes they index into. Clears any prior table. Call at scene entry,
    /// alongside the field-VM `load_field_script` path.
    ///
    /// REF: the prescript bundle the retail `FUN_800252EC` indexes
    /// (`legaia_asset::scene_event_scripts::move_stager_records`).
    pub fn install_field_stagers(&mut self, entry_bytes: &[u8]) {
        self.active_field_fx.clear();
        self.field_stager_bytes = entry_bytes.to_vec();
        self.field_stagers =
            legaia_asset::scene_event_scripts::move_stager_records(entry_bytes).unwrap_or_default();
    }

    /// Spawn one field move-VM stager record by id at `origin` (world units),
    /// mirroring the field-VM op `0x34` sub-3 → `FUN_800252EC(id)` →
    /// `FUN_80021B04` → move VM chain. `id` is the installer argument
    /// (`operand + 1`); it indexes [`Self::field_stagers`] (= the bundle's
    /// `offsets[id]` record). No-ops (returns `false`) when the id is out of
    /// range or no table is installed, matching the retail bounds behaviour.
    /// Tick the spawned effect with [`Self::tick_field_fx`].
    ///
    /// PORT: FUN_800252EC (id → `offsets[id]` record → part-stager spawn)
    pub fn spawn_field_stager(&mut self, id: usize, origin: [i16; 3]) -> bool {
        let Some(part) = self.field_stagers.get(id).cloned() else {
            return false;
        };
        // One stager record = one scene-graph part, staged exactly like a summon
        // part (PC = 2 → record+4). Model base 0 here means a mesh part's
        // `model_index` is its **relative** `model_sel` - the index into the
        // SCENE's TMD pack (retail `DAT_8007C018[model_sel + DAT_8007B6F8]`, where
        // `DAT_8007B6F8 = 5` is the character-mesh prefix and `DAT_8007C018[5..]`
        // is that pack). The host resolves it against the scene pack (the
        // asset-viewer / field-placement `env_tmds` source), NOT the battle
        // `global_tmd_pool`; the `+5` prefix is implicit in indexing the
        // scene-pack list directly. Most field stager records are transform /
        // render-mode (particle / sound) nodes that bind no mesh.
        let scene =
            crate::summon::SummonScene::spawn_parts(&[part], &self.field_stager_bytes, 0, origin);
        self.active_field_fx.push(scene);
        true
    }

    /// Advance every live field move-VM scene-graph effect one frame. Finished
    /// scenes are **kept** (not drained): a finished part stops ticking
    /// ([`crate::summon::SummonScene::tick`] skips finished parts) but holds its
    /// final transform, so a quick-halting mesh effect stays visible at its last
    /// pose rather than vanishing the same frame it halts (which would race the
    /// render). Effects are cleared on scene entry ([`Self::install_field_stagers`])
    /// and, for the debug exerciser, before each `spawn_field_stager`. Faithful
    /// per-effect teardown (when retail removes a finished field effect) is a
    /// future refinement. No-op when none are live.
    pub fn tick_field_fx(&mut self, frame_delta: u16) {
        if self.active_field_fx.is_empty() {
            return;
        }
        let mut scenes = std::mem::take(&mut self.active_field_fx);
        for scene in &mut scenes {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
            };
            scene.tick(&mut host, frame_delta);
        }
        self.active_field_fx = scenes;
    }

    /// Per-part mesh draws across all live field move-VM effects (the visual
    /// parts). The non-visual nodes (`0x4001` sound emitter) never appear here -
    /// see [`Self::active_field_fx_render_nodes`].
    pub fn active_field_fx_part_draws(&self) -> Vec<crate::summon::SummonPartDraw> {
        self.active_field_fx
            .iter()
            .flat_map(|s| s.part_draws())
            .collect()
    }

    /// The live special render-mode nodes across all field move-VM effects -
    /// the `0x4000` particle and `0x4001` **sound-emitter** sentinels, classified
    /// for the renderer / audio host (the sound emitter is *not* a draw). Mirrors
    /// `FUN_80021DF4`'s `+0x5A` split of these nodes off the mesh draw path.
    pub fn active_field_fx_render_nodes(&self) -> Vec<crate::summon::SpecialRenderNode> {
        self.active_field_fx
            .iter()
            .flat_map(|s| s.special_render_nodes())
            .collect()
    }

    /// Spawn a battle move's effect-FX scene-graph at `origin` (world units).
    ///
    /// A move's `0x01..=0x63` on-contact (`+0x12`) / launch (`+0x16`) effect-list
    /// entries each index the `0x801f6324` prototype-pointer table; every such
    /// entry resolves to a summon-format move-VM record (`+0x00 model_sel`,
    /// `+0x02 flags`, `+0x04` bytecode) staged by the shared `FUN_80021B04`
    /// machinery. This parses those records out of the retained battle-action
    /// overlay (PROT 0898) and spawns them as a [`crate::summon::SummonScene`]
    /// with model base `crate::scene::EFFECT_MODEL_LIBRARY_BASE` (the engine's
    /// fixed-library analogue of the retail `gp[0x754] = party_count + 2`; see
    /// that constant's docs), so each mesh part resolves to
    /// `global_tmd_pool[model_sel + 3]` - the PROT 0871 effect-model library,
    /// already resident.
    ///
    /// Returns `false` (no scene spawned) when the move-power table / overlay
    /// isn't installed (disc-free battles), the move id has no power record, or
    /// the move carries no spawnable effect entries. Replaces any in-flight
    /// move-FX scene. Tick with [`Self::tick_move_fx`].
    pub fn spawn_move_fx(&mut self, move_id: u8, origin: [i16; 3]) -> bool {
        let Some(overlay) = self.move_power_overlay.clone() else {
            return false;
        };
        let Some(cat) = self.move_power.as_ref() else {
            return false;
        };
        let Some(fx) = cat.fx_for_move_id(move_id) else {
            return false;
        };
        use legaia_asset::move_power::{self, BATTLE_OVERLAY_BASE, EffectListEntry};

        // Parse ALL prototype records first (full offset set) so each record's
        // move-VM bytecode is bounded by its true packed neighbour, then select
        // the ones this move's Spawn entries reference. Bounding against only
        // this move's subset would over-run each record into the next selected
        // one rather than the next packed one.
        let Some(all_parts) = move_power::parse_effect_proto_records(&overlay) else {
            return false;
        };

        // The file offsets this move's Spawn entries point at (proto VA → file).
        let wanted: std::collections::BTreeSet<usize> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                EffectListEntry::Spawn(_) => e.proto,
                _ => None,
            })
            .filter_map(|va| va.checked_sub(BATTLE_OVERLAY_BASE).map(|o| o as usize))
            .collect();
        if wanted.is_empty() {
            return false;
        }
        let parts: Vec<legaia_asset::summon_overlay::SummonPart> = all_parts
            .into_iter()
            .filter(|p| wanted.contains(&p.record_off))
            .collect();
        if parts.is_empty() {
            return false;
        }
        self.active_move_fx = Some(crate::summon::SummonScene::spawn_parts(
            &parts,
            &overlay,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        // Surface the move's presentation fields for the render / audio layers:
        // the trail/afterimage texpage (`+0x0b`) and the sound cue (`+0x0d`).
        self.active_move_fx_trail_texpage = Some(fx.trail_texpage);
        if fx.sound_cue_id != 0 {
            self.pending_move_fx_cue = Some(fx.sound_cue_id);
        }

        // The high-bit (`0x80`) effect-list entries route to the 2D efect.dat
        // pool (`FUN_801dfdf0`), not the 0x801f6324 scene-graph: spawn each
        // through the effect pool by its 7-bit id (no-op when efect.dat isn't
        // loaded). (Reached only on the scene-graph success path; a move whose
        // lists hold *only* AltEffect entries returns early above with the
        // empty-Spawn-set guard - an documented edge case, rare for FX moves.)
        let alt_ids: Vec<u8> = fx
            .contact_effects
            .iter()
            .chain(fx.launch_effects.iter())
            .filter_map(|e| match e.entry {
                legaia_asset::move_power::EffectListEntry::AltEffect(id) => Some(id),
                _ => None,
            })
            .collect();
        for id in alt_ids {
            self.try_spawn_effect(id, origin, 0);
        }
        true
    }

    /// Take the pending move-FX sound cue id, if [`Self::spawn_move_fx`] set one
    /// this step. The host routes it through `legaia_engine_audio::classify_cue`
    /// (the `FUN_8004fcc8` dispatch) → the SFX ring / voice trigger. Returns
    /// `None` when no cue is pending.
    pub fn take_pending_move_fx_cue(&mut self) -> Option<u8> {
        self.pending_move_fx_cue.take()
    }

    /// The trail / afterimage GP0 texpage word (`0x7700 + id`) for the active
    /// move-FX scene, or `None` when none is playing. The render layer applies
    /// it to the move's streak pass.
    pub fn active_move_fx_trail_texpage(&self) -> Option<u16> {
        self.active_move_fx_trail_texpage
    }

    /// Advance the active move-FX scene one frame through the move VM (the
    /// move-FX sibling of [`Self::tick_summon`]). No-op when none is playing;
    /// drains the scene once every part has finished.
    pub fn tick_move_fx(&mut self, frame_delta: u16) {
        let Some(mut scene) = self.active_move_fx.take() else {
            return;
        };
        {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
            };
            scene.tick(&mut host, frame_delta);
        }
        if !scene.finished() {
            self.active_move_fx = Some(scene);
        } else {
            // Scene drained: drop the trail texpage with it.
            self.active_move_fx_trail_texpage = None;
        }
    }

    /// Per-part render draws for the active move-FX scene's mesh-bearing parts
    /// (empty when none is playing). Each draw's `model_index` indexes
    /// [`Self::global_tmd_pool`] (the PROT 0871 effect-model library).
    pub fn active_move_fx_part_draws(&self) -> Vec<crate::summon::SummonPartDraw> {
        self.active_move_fx
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default()
    }

    /// Take the pending production summon-spawn request, if a player Seru-magic
    /// cast set one this step. Returns `(spell_id, origin)`; the host maps
    /// `spell_id` to the overlay PROT entry (extraction `903 + (spell_id - 0x81)`), loads
    /// it, and calls [`Self::spawn_summon`]. See [`Self::pending_summon_spawn`].
    pub fn take_pending_summon_spawn(&mut self) -> Option<(u8, [i16; 3])> {
        self.pending_summon_spawn.take()
    }

    /// Request a summon spawn for `spell_id` at `origin` if it is a player
    /// Seru-magic id (`0x81..=0x8b`). Idempotent within a step (last cast wins);
    /// no-op for non-summon ids. The retail cast band's overlay-resolve point.
    pub(crate) fn request_summon_spawn(&mut self, spell_id: u8, origin: [i16; 3]) {
        // Base + evolved-Seru summons render their namesake battle_data creature
        // (disc-pinned by `legaia_asset::summon_creatures`); the high block
        // 0x99..=0xA0 is a bespoke mesh not yet supported, so it is not spawned.
        if crate::summon::SERU_SUMMON_IDS.contains(&spell_id)
            || crate::summon::EVOLVED_SUMMON_IDS.contains(&spell_id)
        {
            self.pending_summon_spawn = Some((spell_id, origin));
        }
    }

    /// Drain a pending non-summon move-FX spawn request (the host calls
    /// [`Self::spawn_move_fx`] with it). See [`Self::pending_move_fx_spawn`].
    pub fn take_pending_move_fx_spawn(&mut self) -> Option<(u8, [i16; 3])> {
        self.pending_move_fx_spawn.take()
    }

    /// Request a move-FX spawn for the non-summon move `move_id` at `origin`,
    /// but only when the move-power table is installed and the move's record
    /// carries a spawnable effect entry ([`crate::move_power::MovePowerCatalog::move_has_spawn_fx`]).
    /// No-op otherwise (plain physical hits, disc-free battles with no table).
    /// Idempotent within a step (last cast wins). The move-FX sibling of
    /// [`Self::request_summon_spawn`]; Seru-summon ids go through that instead.
    pub(crate) fn request_move_fx_spawn(&mut self, move_id: u8, origin: [i16; 3]) {
        if self
            .move_power
            .as_ref()
            .is_some_and(|cat| cat.move_has_spawn_fx(move_id))
        {
            self.pending_move_fx_spawn = Some((move_id, origin));
        }
    }

    /// Dev/visualization helper: seat one synthetic active effect into the
    /// pool at `world_pos` (world units) so the effect-pool render bridge can
    /// be exercised by hand. It ages and retires through the normal
    /// [`Self::tick_effects`] lifetime like any spawned effect.
    ///
    /// This is **not** a retail code path - it's a hand-spawn for exercising
    /// the render bridge without driving the action SM. The real catalog (PROT
    /// 0873 `efect.dat`) loads at scene entry, so `ui_element` spawns resolve
    /// to real scripts; use [`Self::try_spawn_effect`] for the production path.
    /// Returns `false` when the pool is full.
    pub fn spawn_debug_effect(&mut self, world_pos: [f32; 3]) -> bool {
        let Some(slot) = self.effect_pool.allocate_master() else {
            return false;
        };
        let m = &mut self.effect_pool.master_slots[slot];
        *m = vm::effect_vm::MasterSlot::default();
        // child_count > 0 marks the slot active for the walker + markers.
        m.child_count = 1;
        m.pos_x = (world_pos[0] * 256.0) as i32;
        m.pos_y = (world_pos[1] * 256.0) as i32;
        m.pos_z = (world_pos[2] * 256.0) as i32;
        true
    }

    /// Spawn effect `ui_id` at `world_pos` / `angle` via the pool, looking
    /// up the script in `self.effect_catalog`. No-op when the catalog is
    /// empty or the id is out of range. Mirrors the retail path through
    /// `FUN_801D8DE8 → FUN_801DFDF8`.
    pub fn try_spawn_effect(&mut self, ui_id: u8, world_pos: [i16; 3], angle: u16) {
        let catalog_ptr: *const vm::effect_vm::EffectCatalog = &self.effect_catalog;
        let pool_ptr: *mut vm::effect_vm::Pool = &mut self.effect_pool;
        let mut host = EffectHostImpl { world: self };
        // SAFETY: EffectHostImpl only reads `world.rng_state`; it never
        // accesses `effect_pool` or `effect_catalog` through the borrow.
        let pool = unsafe { &mut *pool_ptr };
        let catalog = unsafe { &*catalog_ptr };
        let _ = pool.spawn_by_ui_id(&mut host, ui_id, world_pos, angle, catalog);
    }
}
