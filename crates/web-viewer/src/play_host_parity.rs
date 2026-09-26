//! Read-only probes over the play page's live `World`, for the host-parity
//! tests that pair this host against the native window.
//!
//! None of these is a wasm export: they exist so a headless test driving
//! [`LegaiaRuntime`] can assert that a state the native boot installs is
//! installed here too, without reaching into `pub(crate)` fields.

use crate::runtime::LegaiaRuntime;

impl LegaiaRuntime {
    /// Which static-SCUS progression tables the page's world holds - the
    /// state [`World::install_retail_progression_tables`] installs. JSON
    /// object of booleans plus the accessory-passive count; `null` before a
    /// disc is loaded.
    ///
    /// [`World::install_retail_progression_tables`]:
    ///     legaia_engine_core::world::World::install_retail_progression_tables
    pub fn debug_progression_tables_json(&self) -> String {
        let Some(h) = self.scene_host.as_ref() else {
            return "null".into();
        };
        let w = &h.world;
        let t = &w.party.level_up_tracker;
        serde_json::json!({
            "xp_corrections": t.xp_corrections.is_some(),
            "growth": t.growth_tables.is_some(),
            "victory_pose": w.tables.victory_pose_table.is_some(),
            "xa_cue_durations": w.audio.xa_cue_durations.is_some(),
            "magic_xp": w.tables.magic_xp_thresholds.is_some(),
            "accessory_passives": w.tables.accessory_passives.len(),
        })
        .to_string()
    }

    /// The engine camera's retail globals, the state
    /// a scene entry resets (`Camera::reset_globals_for_scene_entry`).
    pub fn debug_camera_globals(&self) -> Vec<i32> {
        self.camera.globals.0.to_vec()
    }

    /// Offset every scene-entry reset axis of the camera globals away from
    /// its reset value, so a test can tell "reset on this entry" from "was
    /// never moved". Test support only; the page never calls it.
    pub fn debug_perturb_camera_globals(&mut self) {
        use legaia_engine_core::camera::RetailCamGlobals;
        for axis in RetailCamGlobals::FIELD_RESET_AXES {
            self.camera.globals.0[axis] = RetailCamGlobals::FIELD_RESET.0[axis] + 0x123;
        }
    }

    /// The lead roster record's live battle window (`+0x110..`), the fields
    /// a Muscle Dome contest stages its fighter from: `{ "agl", "int",
    /// "udf", "ldf", "hp", "hp_max" }`, `null` with no lead.
    pub fn debug_lead_live_stats_json(&self) -> String {
        let Some(lead) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.party.roster.members.first())
        else {
            return "null".into();
        };
        let l = lead.live_stats();
        let h = lead.hp_mp_sp();
        serde_json::json!({
            "agl": l.agl, "int": l.int, "udf": l.udf, "ldf": l.ldf,
            "hp": h.hp_cur, "hp_max": h.hp_max,
        })
        .to_string()
    }
}
