//! `FieldHaltHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldHaltHost for FieldHostImpl<'a> {
    // Op `0x43` sub-0/1/A/B: the scripted arc (`FUN_801D25EC`). An actor the
    // engine cannot place (the scene system context) gets no arc; its halt
    // stays raised exactly as before the arc channel existed.
    fn arc_target_halted(&self, _ctx: &FieldCtx, ext: Option<u8>) -> bool {
        // The arm's acquire refuses an actor still mid arc
        // (`0x801DF3A4..0x801DF3CC`); the VM retries the op next frame.
        self.world.script_arc_target_halted(ext)
    }
}
