//! FMV (STR / MDEC) beats on the play page.
//!
//! The field VM arms a movie by parking the world in `SceneMode::Cutscene`
//! with `cutscene.active_fmv` set. The native window plays the movie through
//! `crates/mdec` + the XA lane (`window/str_player.rs`,
//! `engine-shell/src/cutscene_av.rs`); this module is the browser twin.
//! [`LegaiaRuntime::service_cutscene_fmv`] runs once per
//! [`LegaiaRuntime::tick_frame`] and returns the label of the scene the
//! post-movie hand-off entered (empty when none did).

use legaia_engine_core::world::SceneMode;

use crate::runtime::LegaiaRuntime;

/// Per-runtime movie state. Empty until a movie path lands.
#[derive(Default)]
pub(crate) struct FmvState {}

impl LegaiaRuntime {
    /// Service an armed FMV beat. Today this is the browser auto-skip: the
    /// movie is finished the frame it arms (the 3D cutscene / field resumes,
    /// minus the movie) and the post-movie scene hand-off still applies.
    pub(crate) fn service_cutscene_fmv(&mut self) -> String {
        let mut fmv_handoff_scene = String::new();
        let _ = &mut self.fmv;
        let Some(host) = self.scene_host.as_mut() else {
            return fmv_handoff_scene;
        };
        if host.world.mode == SceneMode::Cutscene && host.world.cutscene.active_fmv.is_some() {
            host.world.finish_cutscene();
            // Skipping the *movie* is not skipping the *hand-off*. Retail's
            // master dispatch writes a next-scene label after playback
            // (`town01` -> fmv 1 -> `town0b`), so auto-skipping without this
            // left the page in the trigger scene - a different place from
            // where the other two hosts land. Same shared kernel, same
            // one-shot `World::take_finished_fmv` edge.
            if let Some(outcome) = host.apply_pending_fmv_handoff() {
                if let legaia_engine_core::scene::FmvHandoffOutcome::Entered { scene, .. } =
                    &outcome
                {
                    fmv_handoff_scene = (*scene).to_string();
                }
                web_sys::console::log_1(&format!("cutscene: {outcome}").into());
            }
        }
        fmv_handoff_scene
    }
}
