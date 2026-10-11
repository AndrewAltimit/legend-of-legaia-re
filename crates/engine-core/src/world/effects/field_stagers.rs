//! Field effect stagers: install, spawn, tick and the part-draw / render-node
//! projections.

use super::*;

impl World {
    /// Install the current scene's field move-VM stager table from the scene's
    /// event-scripts container bytes (a `SceneEventScripts` /
    /// `SceneScriptedAssetTable` entry). Parses the prescript records as
    /// summon-format move-VM stagers and retains both the parsed records and the
    /// bundle bytes they index into. Clears any prior table. Call at scene entry,
    /// alongside the field-VM `load_field_script` path.
    ///
    /// REF: the prescript bundle the retail `FUN_800252EC` indexes
    /// (`legaia_scene_formats::scene_event_scripts::move_stager_records`).
    pub fn install_field_stagers(&mut self, entry_bytes: &[u8]) {
        self.props.active_fx.clear();
        self.props.stager_bytes = entry_bytes.to_vec();
        self.props.stagers =
            legaia_scene_formats::scene_event_scripts::move_stager_records(entry_bytes)
                .unwrap_or_default();
    }

    /// Spawn one field move-VM stager record by id at `origin` (world units),
    /// mirroring the field-VM op `0x34` sub-3 → `FUN_800252EC(id)` →
    /// `FUN_80021B04` → move VM chain. `id` is the installer argument
    /// (`operand + 1`); it indexes [`crate::world::FieldPropState::stagers`] (= the bundle's
    /// `offsets[id]` record). No-ops (returns `false`) when the id is out of
    /// range or no table is installed, matching the retail bounds behaviour.
    /// Tick the spawned effect with [`Self::tick_field_fx`].
    ///
    /// PORT: FUN_800252EC (id → `offsets[id]` record → part-stager spawn)
    pub fn spawn_field_stager(&mut self, id: usize, origin: [i16; 3]) -> bool {
        let Some(part) = self.props.stagers.get(id).cloned() else {
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
        let scene = legaia_engine_effects::summon::SummonScene::spawn_parts(
            &[part],
            &self.props.stager_bytes,
            0,
            origin,
        );
        self.props.active_fx.push(scene);
        true
    }

    /// Advance every live field move-VM scene-graph effect one frame. Finished
    /// scenes are **kept** (not drained): a finished part stops ticking
    /// ([`legaia_engine_effects::summon::SummonScene::tick`] skips finished parts) but holds its
    /// final transform, so a quick-halting mesh effect stays visible at its last
    /// pose rather than vanishing the same frame it halts (which would race the
    /// render). Effects are cleared on scene entry ([`Self::install_field_stagers`])
    /// and, for the debug exerciser, before each `spawn_field_stager`. Faithful
    /// per-effect teardown (when retail removes a finished field effect) is a
    /// future refinement. No-op when none are live.
    pub fn tick_field_fx(&mut self, frame_delta: u16) {
        let channel_delta = self.effect_channel_delta();
        if self.props.active_fx.is_empty() {
            return;
        }
        let mut scenes = std::mem::take(&mut self.props.active_fx);
        for scene in &mut scenes {
            let mut host = MoveVmHostImpl {
                world: self,
                current_slot: None,
                deferred_writes: std::collections::BTreeMap::new(),
                field_record_words: None,
                child_spawns: Vec::new(),
            };
            scene.channel_delta = channel_delta;
            scene.tick(&mut host, frame_delta);
        }
        self.props.active_fx = scenes;
    }

    /// Per-part mesh draws across all live field move-VM effects (the visual
    /// parts). The non-visual nodes (`0x4001` sound emitter) never appear here -
    /// see [`Self::active_field_fx_render_nodes`].
    pub fn active_field_fx_part_draws(&self) -> Vec<legaia_engine_effects::summon::SummonPartDraw> {
        self.props
            .active_fx
            .iter()
            .flat_map(|s| s.part_draws())
            .collect()
    }

    /// The live special render-mode nodes across all field move-VM effects -
    /// the `0x4000` particle and `0x4001` **sound-emitter** sentinels, classified
    /// for the renderer / audio host (the sound emitter is *not* a draw). Mirrors
    /// `FUN_80021DF4`'s `+0x5A` split of these nodes off the mesh draw path.
    pub fn active_field_fx_render_nodes(
        &self,
    ) -> Vec<legaia_engine_effects::summon::SpecialRenderNode> {
        self.props
            .active_fx
            .iter()
            .flat_map(|s| s.special_render_nodes())
            .collect()
    }
}
