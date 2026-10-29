//! The command surface's SFX ring push: one [`BattleUiCue`] a frame onto
//! the ring both hosts drain. The cue kernels are
//! `legaia_engine_battle_loop::ui_cues`.

use crate::world::World;
use legaia_engine_battle_loop::ui_cues::*;
use legaia_engine_system::sound_state::SfxRingOp;

impl World {
    /// Put one command-surface blip on the SFX ring both hosts drain.
    pub(in crate::world) fn push_battle_ui_cue(&mut self, cue: BattleUiCue) {
        self.audio.sfx_ring_ops.push(SfxRingOp::Push(cue.ring_id()));
    }
}
