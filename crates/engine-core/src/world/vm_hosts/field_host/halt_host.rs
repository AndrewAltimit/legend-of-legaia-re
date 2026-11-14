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

    // The spawn bracket `*(_DAT_801C6EA4) + 8`: up for the scene-entry
    // spawn slices (`FUN_8003AEB0`, `0x8003B73C..0x8003B928`), the object
    // binds' prologues inside it, and the `4C D2` / `4C C3` re-spawns -
    // every slice the port runs under `entry_prerun` or `object_prerun`.
    fn spawn_bracket(&self) -> bool {
        self.world.field_vm.entry_prerun || self.world.field_vm.object_prerun
    }
}
