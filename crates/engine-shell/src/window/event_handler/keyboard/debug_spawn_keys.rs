//! The debug spawn keys: hand-spawned effect markers, the Gimard summon
//! scene-graph, a battle move's effect scene-graph and a field stager
//! effect - probes for the render bridges, not player surfaces.

use super::super::super::*;

impl PlayWindowApp {
    /// `F5` / `F` / `G` / `H` / `J`: the debug effect spawns. `true` when one took the key.
    pub(super) fn debug_spawn_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        self.key_debug_effect(code, state)
            || self.key_debug_effect_model(code, state)
            || self.key_debug_summon(code, state)
            || self.key_debug_move_fx(code, state)
            || self.key_debug_field_fx(code, state)
    }

    /// Dev affordance: spawn a debug effect marker at the player so
    /// the effect-pool render bridge can be exercised by hand
    /// before the runtime effect catalog is wired into battle-enter.
    ///
    /// On `F5`, not on `E`, and the rest of this family (`F`/`G`/`H`/`J`)
    /// stays on letters: `E` is bound to R1 in the engine's default pad
    /// table, and an arm here returns before the pad lookup, so a window
    /// arm sitting on a bound key silently deletes that button from the
    /// keyboard. See `keycode_to_name`.
    fn key_debug_effect(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::F5)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let pos = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x as f32,
                        a.move_state.world_y as f32,
                        a.move_state.world_z as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            self.session.host.world.spawn_debug_effect(pos);
            return true;
        }
        false
    }

    /// `F`: seat a synthetic *Tail Fire* effect carrying its `etmd` 3D model.
    fn key_debug_effect_model(&mut self, code: KeyCode, state: ElementState) -> bool {
        // `F`: seat a synthetic *Tail Fire* effect carrying its `etmd`
        // 3D model (global TMD pool index 4, textured by the resident
        // `etim` texels) at the first active actor, exercising the
        // model render path with a fixed model index. The data-driven
        // effect-id -> etmd-model selection is decoded (move-power
        // record `+0x12`/`+0x16` effect-id lists -> the `0x801F6324`
        // prototype table -> record `model_sel` -> `global_tmd_pool
        // [model_sel + 3]`; `legaia_game_tables::move_power::EffectListEntry`
        // + `World::spawn_move_fx`, exercised by the `H` key); `F` is
        // just the simpler fixed-model dev hand-spawn.
        if matches!(code, KeyCode::KeyF)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let pos = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x as f32,
                        a.move_state.world_y as f32,
                        a.move_state.world_z as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            // Prefer the real Gimard *Tail Fire* mesh from the PROT
            // 0871 effect-model library when it's resident; fall back
            // to the PROT 0874 §0 preview stand-in otherwise.
            let model_index = if self
                .session
                .host
                .world
                .global_tmd(
                    legaia_engine_scene_host::scene_host::GIMARD_TAIL_FIRE_MODEL_INDEX as i16,
                )
                .is_some()
            {
                legaia_engine_scene_host::scene_host::GIMARD_TAIL_FIRE_MODEL_INDEX
            } else {
                legaia_engine_scene_host::scene_host::ETMD_TAIL_FIRE_MODEL_INDEX
            };
            self.session
                .host
                .world
                .spawn_debug_effect_model(pos, model_index);
            return true;
        }
        false
    }

    /// `G`: debug-spawn the Gimard *Burning Attack* summon scene-
    /// graph (extraction PROT 0903). Loads the stager overlay, parses
    /// records, and seats a `SummonScene` at the first active actor;
    /// it then animates each frame via `tick_summon` (the move VM)
    /// and renders through the summon-part draw block below. Not the
    /// production cast-band trigger (state 0x29) -- a hand-spawn to
    /// exercise the driver, like the `F`-key static effect spawn.
    fn key_debug_summon(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyG)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            // In battle, debug-spawn the faithful summon creature
            // (Gimard, 0x81) through the battle-creature render path.
            if self.session.host.world.mode == SceneMode::Battle {
                self.spawn_summon_creature(0x81);
                return true;
            }
            const PROT_GIMARD_SUMMON_STAGER: u32 = 903;
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            // Read the stager's TOC-gap LBA footprint, not the
            // indexed sub-region: the stager `.BIN`s over-read into
            // the next entry, so the spawn-site pointer table must be
            // parsed against the trimmed window (mirrors the disc-gated
            // `summon_overlay_real` test's `unique_content_len` trim).
            match self
                .session
                .host
                .index
                .entry_bytes_lba_footprint(PROT_GIMARD_SUMMON_STAGER)
            {
                Ok(bytes) => {
                    let overlay = legaia_overlay_images::summon_overlay::parse(
                        &bytes,
                        legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE,
                    );
                    self.session.host.world.spawn_summon(
                        &overlay,
                        &bytes,
                        legaia_engine_scene_host::scene_host::GIMARD_TAIL_FIRE_MODEL_INDEX,
                        origin,
                    );
                    log::info!(
                        "spawned Gimard summon: {} parts at {origin:?}",
                        overlay.parts.len()
                    );
                }
                Err(e) => log::warn!("summon spawn: read summon stager PROT entry: {e:#}"),
            }
            return true;
        }
        false
    }

    /// `H`: debug-spawn a battle move's effect-FX scene-graph (the
    /// move-power `0x801f6324` prototype records, summon-format
    /// move-VM parts spawned through `FUN_80021B04`). Battle only -
    /// the move-power table + overlay are battle-context. Move id
    /// 0x06 is the worked example with library-mesh Spawn entries
    /// (0x27 / 0x28); its parts resolve into the effect-model library
    /// `global_tmd_pool[model_sel + 3]` and animate via `tick_move_fx`,
    /// rendered by the move-FX draw block below.
    fn key_debug_move_fx(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyH)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode != SceneMode::Battle {
                log::info!("move-FX spawn (H) is battle-only");
                return true;
            }
            // Enumerate every move the parsed move-power table can render
            // as a 3D scene-graph and cycle through them across presses
            // (data-driven, so the preview reflects the real overlay
            // rather than one hard-coded id). The 0x06 worked example -
            // library-mesh Spawn entries 0x27/0x28 resolving into
            // `global_tmd_pool[model_sel + 3]` - is the starting point
            // when present.
            let spawnable = self
                .session
                .host
                .world
                .tables
                .move_power
                .as_ref()
                .map(|c| c.spawnable_move_ids())
                .unwrap_or_default();
            if spawnable.is_empty() {
                log::info!(
                    "move-FX spawn (H): move-power table not installed / no spawnable moves"
                );
                return true;
            }
            use std::sync::atomic::{AtomicUsize, Ordering};
            static MOVE_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
            let start = spawnable.iter().position(|&id| id == 0x06).unwrap_or(0);
            let n = MOVE_FX_CYCLE.fetch_add(1, Ordering::Relaxed);
            let slot = (start + n) % spawnable.len();
            let move_fx_id = spawnable[slot];
            log::info!(
                "move-FX preview {}/{}: move {move_fx_id:#04x} (spawnable {:#04x?})",
                slot + 1,
                spawnable.len(),
                spawnable
            );
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            if self.session.host.world.spawn_move_fx(move_fx_id, origin) {
                log::info!(
                    "spawned move-FX for move {move_fx_id:#04x}: {} mesh parts at {origin:?}",
                    self.session.host.world.active_move_fx_part_draws().len()
                );
                // Consume the surfaced presentation fields: the trail
                // texpage (render layer's streak pass) and the sound cue
                // (routed through the FUN_8004fcc8 dispatch decode). The
                // host has no battle SFX bank wired yet, so the cue is
                // resolved + logged rather than fired through the SPU.
                if let Some(trail) = self.session.host.world.active_move_fx_trail_texpage() {
                    log::info!("  move-FX trail texpage = {trail:#06x}");
                }
                if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                    let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                    log::info!("  move-FX sound cue {cue:#04x} -> {dispatch:?}");
                }
            } else {
                log::info!(
                    "move-FX spawn for move {move_fx_id:#04x} produced no parts \
                     (table not installed / no spawnable entries)"
                );
            }
            return true;
        }
        false
    }

    /// `J`: debug-spawn one field move-VM scene-graph effect from the
    /// current scene's prescript stager table (the op-0x34-sub-3 /
    /// `FUN_800252EC` path), cycling through the table across presses.
    /// Exercises the field-FX tick/draw wiring without waiting for the
    /// field VM to execute an op-0x34-sub-3; the production trigger is
    /// `FieldHostImpl::effect_anim_trigger`.
    fn key_debug_field_fx(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyJ)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let count = self.session.host.world.props.stagers.len();
            if count == 0 {
                log::info!("field-FX spawn (J): no prescript stager table for this scene");
                return true;
            }
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            use std::sync::atomic::{AtomicUsize, Ordering};
            static FIELD_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
            let id = FIELD_FX_CYCLE.fetch_add(1, Ordering::Relaxed) % count;
            // Debug ergonomics: isolate one record per press (the
            // production op-0x34-sub-3 path lets them stack).
            self.session.host.world.props.active_fx.clear();
            if self.session.host.world.spawn_field_stager(id, origin) {
                let mesh: Vec<usize> = self
                    .session
                    .host
                    .world
                    .active_field_fx_part_draws()
                    .iter()
                    .map(|d| d.model_index)
                    .collect();
                let nodes: Vec<_> = self
                    .session
                    .host
                    .world
                    .active_field_fx_render_nodes()
                    .iter()
                    .map(|n| n.mode)
                    .collect();
                log::info!(
                    "field-FX spawn {}/{count} at {origin:?}: {} mesh parts (model_index {mesh:?}), {} render-mode nodes {nodes:?}",
                    id + 1,
                    mesh.len(),
                    nodes.len(),
                );
            }
            return true;
        }
        false
    }
}
