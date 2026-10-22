//! Summon scene-graph spawn, tick and part-draw projection, plus the pending
//! summon-spawn request.

use super::*;

impl World {
    /// Spawn a Seru-magic summon scene-graph from a parsed stager overlay (e.g.
    /// extraction PROT 0903, Gimard *Burning Attack*) at `origin` (world units).
    /// `record_bytes` is the overlay's raw bytes (the buffer `overlay` was parsed
    /// from); `model_base` is the pool index a part's `model_sel == 0` resolves to
    /// (the summon's mesh-set base, e.g. [`legaia_engine_scene_host::scene_host::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    /// Replaces any in-flight summon. Tick it with [`Self::tick_summon`].
    ///
    /// NOTE this drives the engine's **move-VM scene-graph stand-in**
    /// ([`legaia_engine_effects::summon::SummonScene`]), not the faithful player-summon render. A
    /// live trace resolved that retail draws the player summon as an ordinary
    /// battle actor via the per-object TRS-keyframe path `FUN_80048A08` /
    /// `FUN_8004998C` (ported in [`legaia_engine_vm::anim_vm`]); see the
    /// `SummonScene` module reconciliation note.
    // REF: FUN_80048A08 (faithful player-summon render = battle-actor TRS-keyframe draw)
    pub fn spawn_summon(
        &mut self,
        overlay: &legaia_overlay_images::summon_overlay::SummonOverlay,
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) {
        self.casting.active_summon = Some(legaia_engine_effects::summon::SummonScene::spawn(
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
        let channel_delta = self.effect_channel_delta();
        let Some(mut scene) = self.casting.active_summon.take() else {
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
                field_record_words: None,
                child_spawns: Vec::new(),
            };
            scene.channel_delta = channel_delta;
            scene.tick(&mut host, frame_delta);
        }
        self.queue_battle_part_scrolls(&mut scene.parts);
        // The morph-lane ramp envelope `FUN_80020740`, run by the part tick
        // on every part whose `+0x10` carries the op-`0x0A` bit `0x1000` -
        // the step `tick_move_fx` takes for the effect-script scenes. PROT
        // 0903's breath (the PROT 0898 prototype `0x801F5B28`) arms three
        // lanes on its rest mesh, a 52-unit seed, and grows to the flame
        // only through them. The step is the move-VM ramp ratio, as for the
        // effect-script scenes below, not the field cadence `frame_step`.
        let step = self.move_vm.ramp_ratio.max(1);
        for part in &mut scene.parts {
            if !part.finished
                && part.state.flags & vm::move_buffer::STATUS_FLAG_ENVELOPE_ACTIVE != 0
            {
                vm::vdf_morph::envelope_tick_actor(&mut part.state, step);
            }
        }
        if !scene.finished() {
            self.casting.active_summon = Some(scene);
        }
    }

    /// The part tick's channel scale, `DAT_1F800393 * DAT_1F80037D`: the live
    /// frame step (`MoveVmGlobals::ramp_ratio`, the `DAT_1F800393` stand-in)
    /// times the rate byte's boot value.
    pub(super) fn effect_channel_delta(&self) -> u16 {
        u16::from(self.move_vm.ramp_ratio.max(1))
            * legaia_engine_effects::summon::RETAIL_CHANNEL_DELTA
    }

    /// Per-part render draws for the active summon's mesh-bearing parts (empty
    /// when no summon is playing). Each draw's `model_index` indexes
    /// [`Self::global_tmd_pool`]. See [`legaia_engine_effects::summon::SummonScene::part_draws`]
    /// for the faithful-tick / interpreted-transform boundary.
    pub fn active_summon_part_draws(&self) -> Vec<legaia_engine_effects::summon::SummonPartDraw> {
        self.casting
            .active_summon
            .as_ref()
            .map(|s| s.part_draws())
            .unwrap_or_default()
    }

    /// Take the pending production summon-spawn request, if a player Seru-magic
    /// cast set one this step. Returns `(spell_id, origin)`; the host maps
    /// `spell_id` to the overlay PROT entry (extraction `903 + (spell_id - 0x81)`), loads
    /// it, and calls [`Self::spawn_summon`]. See [`crate::world::CastFxState::pending_summon_spawn`].
    pub fn take_pending_summon_spawn(&mut self) -> Option<(u8, [i16; 3])> {
        self.casting.pending_summon_spawn.take()
    }

    /// The pending summon-spawn request without taking it - for a host that
    /// takes it only once it has seated the creature.
    pub fn pending_summon_spawn(&self) -> Option<(u8, [i16; 3])> {
        self.casting.pending_summon_spawn
    }

    /// Request a summon spawn for `spell_id` at `origin` if it is a player
    /// summon id (`0x81..=0xA0`). Idempotent within a step (last cast wins);
    /// no-op for non-summon ids. The retail cast band's overlay-resolve point.
    pub(crate) fn request_summon_spawn(&mut self, spell_id: u8, origin: [i16; 3]) {
        // Every player summon seats its cast's own body as slot 7: the
        // archive twin for the base + evolved blocks, the `summon.dat` record
        // for the rest (`legaia_engine_effects::summon::summon_spawn_asset`).
        if legaia_engine_effects::summon::PLAYER_SUMMON_IDS.contains(&spell_id) {
            self.casting.pending_summon_spawn = Some((spell_id, origin));
        }
    }
}
