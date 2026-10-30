//! The ways into a pack map from the retail game: an entrance rectangle in a
//! retail field scene (`[[entrances]]` of `maps/<id>.toml`) and the pause
//! menu's Travel page.
//!
//! Both end in [`SceneHost::enter_expansion_map`]. The world decides when an
//! entrance fires (`World::step_expansion_entrance`, which holds every gate);
//! this step only takes the hit, or the Travel request, and enters the map.

use super::*;

impl SceneHost {
    /// One tick of the pack's ways in: a Travel request the pause menu left,
    /// else the entrance the player just stepped onto. `None` when neither
    /// moved the player (always, with no pack active). An entry that fails
    /// is logged and leaves the scene running.
    pub(crate) fn step_expansion_entrances(&mut self) -> Option<SceneTickEvent> {
        let (map, spawn) = match self.world.take_expansion_travel() {
            Some(t) => t,
            None => {
                let hit = self.world.step_expansion_entrance()?;
                (hit.map, hit.spawn)
            }
        };
        if let Err(e) = self.enter_expansion_map(&map, &spawn) {
            log::warn!("expansion: entering map '{map}' failed: {e:#}");
            return None;
        }
        let name = self
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        Some(SceneTickEvent::SceneEntered { name })
    }
}
