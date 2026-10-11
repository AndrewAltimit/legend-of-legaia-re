//! The hand-off between the panel and the save screen it opens.

use super::*;

/// Which of the save/load UI's two completion screens released the flash.
///
/// The menu overlay seeds `_DAT_8007B43C` differently on each and its
/// completion path adds [`FLASH_COUNTER_DONE_STEP`]; the field-side actor then
/// reads the sum back as its exit phase. That makes this the *return value* of
/// the save screen, which is why it is an enum here and not a bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveScreenOutcome {
    /// Seed [`FLASH_COUNTER_SEED_A`] - exit phase 6, handler `0x29`.
    HandlerA,
    /// Seed [`FLASH_COUNTER_SEED_B`] - exit phase 7, handler `0x2B`.
    HandlerB,
}

impl SaveScreenOutcome {
    /// The counter the menu side leaves for the field side, seed plus step.
    pub fn release_counter(self) -> i32 {
        match self {
            SaveScreenOutcome::HandlerA => FLASH_RELEASE_COUNTER,
            SaveScreenOutcome::HandlerB => FLASH_RELEASE_COUNTER_ALT,
        }
    }
}

/// The live save-screen hand-off `FUN_801D841C` made, while it is outstanding.
///
/// Retail's actor is a node on the system pool at `_DAT_8007C34C` whose `+0xC`
/// handler is `0x80024190`; here it is the record of the spawn plus the one
/// field the spawner writes. The engine reaches the memory-card UI as host
/// screen state rather than by paging a code overlay under a running field
/// session, so what the host owns is the *hand-off*, not a second copy of
/// [`crate::field_save_screen_actor`]'s eleven-state sequencer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveScreenHandoff {
    /// `FUN_80020DE0`'s first argument, `0x800706BC`.
    pub descriptor: u32,
    /// Its second, `_DAT_8007C34C`.
    pub pool: u32,
    /// `actor[+0x5C] != 0` - the save side. `FUN_801D841C` writes `1`; the
    /// fill-fade sibling `FUN_801EE5D4` spawns the same descriptor and leaves
    /// it zero, which is the load side.
    pub is_save: bool,
}
