//! The menu overlay's five-step **open sequence** (`FUN_801DAD6C`).
//!
//! A jump-table state machine over `DAT_801E46AC`, five entries at overlay
//! VA `0x801CEE80`, run once per frame while the menu screen is coming up.
//! Any step value at or above five falls straight through to the tail.
//!
//! | step | jump target | what it does |
//! |---|---|---|
//! | 0 | `0x801DADA4` | stage `DAT_801C6EA0` into `DAT_8007B44C`, hand the script at `0x801E4A78` to the actor VM (`FUN_801D6628`), advance |
//! | 1 | `0x801DADC8` | hold while `DAT_8007BB80` is non-zero; otherwise run `FUN_80020DE0` and advance |
//! | 2 | `0x801DADEC` | advance |
//! | 3 | `0x801DADEC` | advance |
//! | 4 | `0x801DAE04` | clear `DAT_801E46A4` and stop advancing |
//!
//! Two things the shape makes explicit and a state-name reading would
//! hide. Steps 2 and 3 share one jump-table target and carry no body at
//! all - they are two idle frames, not two phases. And step 4 is terminal
//! by omission: it is the only arm that does not fall into the shared
//! increment, so the sequence parks there until something else rewrites
//! the step.
//!
//! The tail call `FUN_80031D00` runs on **every** frame, including the
//! ones that hold and the ones past the table - it is outside the switch,
//! not inside any arm.
//!
//! Evidence: `ghidra/scripts/funcs/overlay_menu_801dad6c.txt` and the
//! jump table read out of the as-loaded PROT 0899 image.

/// Steps the jump table covers. A step at or above this runs the tail
/// only.
pub const MENU_OPEN_STEPS: u32 = 5;

/// What one frame of the sequence asks the host to do.
///
/// The tail (`FUN_80031D00`, the window-chrome stage) is not modelled as
/// an effect because it is unconditional - a host runs it after every
/// [`step`] call, whatever comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOpenEffect {
    /// Step 0: publish the staged pointer and start the menu's actor
    /// script on the actor VM.
    StartActorScript,
    /// Step 1, gate open: the deferred upload `FUN_80020DE0` runs.
    RunDeferredUpload,
    /// A frame with no work - steps 2 and 3, and every out-of-range step.
    Idle,
    /// Step 4: clear the sequence's completion flag `DAT_801E46A4`.
    Finish,
}

/// One frame of the open sequence.
///
/// `busy` is retail's `DAT_8007BB80` gate, read only by step 1. Returns
/// the effect for this frame and leaves `step` advanced when the arm
/// falls into the shared increment.
///
/// PORT: FUN_801DAD6C
/// NOT WIRED: the menu host opens its screens directly rather than
/// running the retail open sequence
pub fn step(step_no: &mut u32, busy: bool) -> MenuOpenEffect {
    match *step_no {
        0 => {
            *step_no += 1;
            MenuOpenEffect::StartActorScript
        }
        1 => {
            if busy {
                // The hold arm jumps past the increment, so the step
                // stays at 1 and the upload is retried next frame.
                return MenuOpenEffect::Idle;
            }
            *step_no += 1;
            MenuOpenEffect::RunDeferredUpload
        }
        2 | 3 => {
            *step_no += 1;
            MenuOpenEffect::Idle
        }
        4 => MenuOpenEffect::Finish,
        _ => MenuOpenEffect::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_busy_gate_holds_step_one_without_advancing() {
        let mut s = 1;
        assert_eq!(step(&mut s, true), MenuOpenEffect::Idle);
        assert_eq!(s, 1);
        assert_eq!(step(&mut s, false), MenuOpenEffect::RunDeferredUpload);
        assert_eq!(s, 2);
    }

    #[test]
    fn steps_two_and_three_are_two_idle_frames_not_two_phases() {
        let mut s = 2;
        assert_eq!(step(&mut s, false), MenuOpenEffect::Idle);
        assert_eq!(step(&mut s, false), MenuOpenEffect::Idle);
        assert_eq!(s, 4);
    }

    #[test]
    fn step_four_is_terminal_by_omission() {
        let mut s = 4;
        for _ in 0..3 {
            assert_eq!(step(&mut s, false), MenuOpenEffect::Finish);
            assert_eq!(s, 4, "the terminal arm never reaches the increment");
        }
    }

    #[test]
    fn out_of_range_steps_run_the_tail_only() {
        let mut s = MENU_OPEN_STEPS;
        assert_eq!(step(&mut s, false), MenuOpenEffect::Idle);
        assert_eq!(s, MENU_OPEN_STEPS, "no arm ran, so nothing advanced");
        let mut s = 0xFFFF_FFFF;
        assert_eq!(step(&mut s, true), MenuOpenEffect::Idle);
    }

    #[test]
    fn a_clean_run_reaches_the_finish_arm_in_four_frames() {
        let mut s = 0;
        let seen: Vec<_> = (0..4).map(|_| step(&mut s, false)).collect();
        assert_eq!(
            seen,
            vec![
                MenuOpenEffect::StartActorScript,
                MenuOpenEffect::RunDeferredUpload,
                MenuOpenEffect::Idle,
                MenuOpenEffect::Idle,
            ]
        );
        assert_eq!(step(&mut s, false), MenuOpenEffect::Finish);
    }
}
