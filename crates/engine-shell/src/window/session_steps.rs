//! The play window's named per-tick steps over the shared session kernels
//! (`legaia_engine_session::BootSession`'s frame steps and
//! `MenuRuntime::poll_field_overlay`).
//!
//! Each is the kernel call plus the one thing only this host owns: the boot-UI
//! state a wipe hands the frame to, the cue voice, the boot SCUS read behind
//! the Trade row, the summon creature's mesh bind. The browser play page names
//! the same steps over the same kernels (`poll_game_over`, `poll_field_shop`,
//! `tick_world_effects`, `tick_muscle_time_meter`), which is what
//! `scripts/ci/check-ui-host-drift.py` pairs on the two frame paths.

use super::*;

impl PlayWindowApp {
    /// Party wipe: start the return-to-title hold, which owns the frame from
    /// here (the boot-UI arm skips the scene tick while any boot UI is up).
    /// The probe and its one-shot are the session's
    /// (`BootSession::take_party_wipe`).
    pub(super) fn poll_game_over(&mut self) {
        if let Some(hold) = self.session.take_party_wipe(self.boot_ui.is_active()) {
            self.boot_ui = BootUiState::GameOver(hold);
        }
    }

    /// A field-VM shop (op `0x49` sub-0) or casino prize counter (sub-7)
    /// opened this tick: hand the player into it, take the screen's first
    /// step on no edge and queue its blip - the session's step
    /// (`BootSession::poll_field_overlay` over `MenuRuntime::poll_field_overlay`),
    /// which the browser play page runs at the same point of its frame. The
    /// field VM stays suspended (op-`0x49` Armed) until the player leaves.
    /// Returns whether one opened.
    pub(super) fn poll_field_shop(&mut self) -> bool {
        self.session.poll_field_overlay(&mut self.menu_runtime)
    }

    /// The tick's effect step: seat a cast's summon creature, then the
    /// session's move-FX spawn + scene-graph tick
    /// (`BootSession::step_effect_scene_graphs`).
    ///
    /// The faithful render of a player summon cast (spell id `0x81..=0xA0`)
    /// is the cast's own body drawn through the enemy animation pipeline (the
    /// namesake `battle_data` creature for `0x81..=0x95`, the cast's
    /// `summon.dat` record above that), so the request is seated as a battle
    /// creature rather than the move-VM scene-graph stand-in.
    pub(super) fn tick_world_effects(&mut self) {
        if let Some((spell_id, _origin)) = self.session.host.world.take_pending_summon_spawn() {
            self.spawn_summon_creature(spell_id);
        }
        let _ = self.session.step_effect_scene_graphs();
    }
}
