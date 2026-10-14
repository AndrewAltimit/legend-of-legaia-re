use super::*;

/// Test-only probes for the disc-gated battle-overlay oracle
/// (`tests/battle_overlay_parity.rs`). Native-only so the wasm export surface
/// the page consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// Force entry into a real scripted battle off the current scene's own
    /// MAN formation table (the op-`0x3E` boss-entry path), so a test reaches
    /// `SceneMode::Battle` without walking the RNG. Tries every plausible
    /// formation row and returns `true` once the world is in battle.
    pub fn debug_start_test_battle(&mut self) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let armed = (0u8..32).any(|row| host.world.trigger_scripted_battle(row));
        if !armed {
            return false;
        }
        // The pending carrier battle is drained by the field tick, and the
        // field-to-battle intro transition holds the mode in Field for its
        // 132 display frames before the flip.
        for _ in 0..220 {
            if self
                .scene_host
                .host()
                .is_some_and(|h| h.world.mode == SceneMode::Battle)
            {
                return true;
            }
            let _ = self.tick_frame();
        }
        self.scene_host
            .host()
            .is_some_and(|h| h.world.mode == SceneMode::Battle)
    }

    /// The battle actor slots of the party - an actor with a battle clip
    /// player and no monster id, the same party test
    /// `World::battle_weapon_trail_draws` makes. Empty outside battle.
    pub fn debug_battle_party_slots(&self) -> Vec<usize> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        if host.world.mode != SceneMode::Battle {
            return Vec::new();
        }
        host.world
            .actors
            .iter()
            .enumerate()
            .filter(|(_, a)| a.battle_monster_id.is_none() && a.battle_animation.is_some())
            .map(|(i, _)| i)
            .collect()
    }

    /// How many weapon-trail draws (`World::battle_weapon_trail_draws`) the
    /// live battle plans this frame - the count the page's screen-FX pass
    /// hands to `legaia_render_kernels::battle_trail::weapon_trail_prims`.
    pub fn debug_battle_weapon_trail_draw_count(&self) -> usize {
        self.scene_host
            .host()
            .map(|h| h.world.battle_weapon_trail_draws().len())
            .unwrap_or(0)
    }

    /// Whether the live cast drew a Theeder beam packet this tick
    /// (`World::theeder_draw`) - the packet the page's screen-FX pass hands
    /// to `legaia_engine_ui::cast_theeder::theeder_prims`.
    pub fn debug_battle_theeder_packet_live(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.theeder_draw().is_some())
    }

    /// Raise a system-flag-bank bit on the live world, the way a field-VM
    /// `0x5x` SET op does.
    ///
    /// The oracles use it to put a disc-side precondition in place without
    /// replaying the script that would - e.g. flag
    /// `legaia_engine_battle::battle_tutorial::TUTORIAL_ARM_FLAG`, which town01's
    /// Tetsu record sets two ops before its battle-entry op. Deliberately a
    /// raw flag poke rather than a "start the tutorial" hook: a test that can
    /// only ask for the outcome cannot tell a wired condition from a shim.
    pub fn debug_system_flag_set(&mut self, idx: u16) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        host.world.system_flag_set(idx);
        true
    }

    /// Read a system-flag-bank bit off the live world (`false` with no scene).
    pub fn debug_system_flag_test(&self, idx: u16) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.system_flag_test(idx))
    }
}
