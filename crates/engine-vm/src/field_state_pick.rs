//! The field overlay's debug-gated actor state pick: one entry of the
//! per-state handler table at `0x801F33B8`, which pushes the actor's current
//! state onto the scene record and installs either the normal successor state
//! `0x30` or the debug shortcut state `0x13`.
//!
//! The port of `FUN_801f1f4c` is [`state_pick`], which carries the `PORT` tag
//! and its own wiring disclosure. It is deliberately not repeated at module
//! level - a `//!  PORT:` line makes the whole file a second, coarser anchor for
//! the same address, with no disclosure of its own.
//!
//! # Provenance
//!
//! PROT entry `0897_xxx_dat` (the field overlay), slot-A base `0x801CE818`,
//! file offset `0x23734` - comfortably inside the overlay's own `0x25000`
//! bytes of content, so this is field code and not the PROT 0898 tail the
//! extraction over-reads past that boundary.
//!
//! 23 instructions, no stack frame, `jr ra` at `0x801F1FCC` with a second
//! `jr ra` immediately after at `0x801F1FD4` and the next function's
//! `addiu sp, sp, -0x18` prologue at `0x801F1FDC`. Like its siblings it is
//! reached through a table rather than a `jal`: the word `0x801F1F4C` sits at
//! VA `0x801F33D0` in the same image, the seventh slot of a run of
//! `0x801F1xxx` handler pointers that starts at `0x801F33B8`.
//!
//! # Globals it reads
//!
//! | Global | Meaning |
//! |---|---|
//! | `_DAT_8007B450` | field-VM op-`0x49` `STATE_RESUME` slot: non-zero while a script is parked on that op |
//! | `_DAT_8007B98C` | the build's debug-mode word |
//! | `_DAT_8007B850` | the packed per-frame pad mask; bit `0x100` is the skip/confirm press |
//! | `_DAT_801C6EA4` | the resident scene pointer |
//!
//! The gate order in the disassembly is:
//!
//! ```text
//! if (_DAT_8007B450 != 0)                       -> state 0x30
//! else if (_DAT_8007B98C == 0)                  -> state 0x30
//! else if ((_DAT_8007B850 & 0x100) == 0)        -> state 0x30
//! else                                          -> state 0x13
//! ```
//!
//! Both arms then perform the *same* three writes before storing the state, so
//! the only thing the debug gate changes is which state is installed:
//!
//! * `scene[+0x2E] = -1`
//! * `scene[+0x40] = actor[+0x50]` (the outgoing state, saved for the return)
//! * `actor[+0x50] = state; actor[+0x54] = 0`
//!
//! Note the guard is a **conjunction**: debug mode alone is not enough, the
//! pad bit has to be held on the frame the handler runs. That is why the
//! shortcut is invisible in ordinary play even on a debug build.
//!
//! # Not wired
//!
//! The blocker is the **route to this slot**, and the actor pair is not part
//! of it.
//!
//! `engine-core` does model `+0x50` / `+0x54`: they are `Actor::state_50` and
//! `Actor::state_54`, documented at those offsets, and `man_load_actor_reset`
//! already clears the second on a fresh submode driver. The table this handler
//! sits in is dispatched live, too - slot `7` of the 52-entry
//! `PTR_FUN_801F33B4`, whose port is `World::tick_submode_screen`'s `run_slot`
//! over [`crate::baka_hub_actors::slot`].
//!
//! What is missing is anything that puts `7` in an actor's `+0x50`, and the
//! reason is not the one this note used to give. The op-`0x49` payload **is**
//! carried: `slot_for_op49_sub_op` indexes retail's own sub-op table
//! ([`crate::baka_hub_actors::OP49_SUBOP_SLOTS`] at `0x801F33A4`) with the
//! parked operand's first byte, exactly as the enter half does at
//! `0x801F145C`. The route cannot come from there for a stronger reason: that
//! table's fourteen entries name slots `0x21`..`0x33` and `-1`, and **none of
//! them is `7`**, so no `49 <sub_op>` reaches this handler in retail either.
//!
//! Slot `7` is a **return** state, and the writer is the submode enter half
//! itself - the routine whose table read at `0x801F145C` the port already
//! cites. Scanning every based overlay image for a store of the immediate `7`
//! into `+0x50` finds exactly one site, at `0x801F140C`, and its surroundings
//! are the same four writes this handler makes:
//!
//! ```text
//! 801f13f4  sh   v0,0x2e(v1)     ; scene[+0x2E] = -1
//! 801f1400  sh   v0,0x40(v1)     ; scene[+0x40] = actor[+0x50]  (outgoing)
//! 801f1404  addiu v0,zero,7
//! 801f140c  sh   v0,0x50(s4)     ; actor[+0x50] = 7
//! 801f141c  sh   zero,0x54(s4)
//! ```
//!
//! Then, when `_DAT_8007B450` names a sub-op the `0x801F33A4` table gives a
//! handler for, the same routine immediately does it **again** at
//! `0x801F1474..0x801F14AC` - saving the `7` it just installed into
//! `scene[+0x40]` and overwriting `+0x50` with the table's slot. So `7` is what
//! the screen returns to: the hand-back restores `+0x50` from `scene[+0x40]`,
//! this handler runs once, and installs `0x30` (or `0x13` on the debug
//! shortcut).
//!
//! That is what the port has no room for. `field_submode_screen` installs the
//! table's slot directly, collapsing a two-step chain into one, and carries no
//! `scene[+0x40]` return slot for the intermediate state to be parked in. The
//! remaining work is the return slot and the debug-mode word, not a dispatch
//! arm.
//!
//! The `+0x2E` / `+0x40` pair the handler writes is **not** homeless: it is
//! `DAT_801C6EA4`, the submode cursor context, and the port models both cells
//! as `HubGrid::handback` / `HubGrid::stashed_state`
//! ([`crate::baka_hub_actors::HubGrid`]), which every hand-back already
//! writes. What is missing is the use of `+0x40` as a **return** slot - the
//! enter half parking `7` there, and a hand-back restoring `+0x50` from it -
//! plus the build's debug-mode word `_DAT_8007B98C`, which the engine's input
//! path does not carry, so even reached, the handler could only ever pick
//! [`STATE_NORMAL`].

/// The state installed on every non-debug path.
pub const STATE_NORMAL: u16 = 0x30;
/// The state installed when debug mode is on *and* the pad bit is held.
pub const STATE_DEBUG: u16 = 0x13;
/// Pad-mask bit the debug arm requires (`_DAT_8007B850 & 0x100`).
pub const PAD_DEBUG_BIT: u32 = 0x100;

/// The four globals the routine reads, gathered so the decision is a pure
/// function of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatePickInputs {
    /// `_DAT_8007B450` - the op-`0x49` `STATE_RESUME` slot.
    pub script_resume_slot: u32,
    /// `_DAT_8007B98C` - the debug-mode word.
    pub debug_mode: u32,
    /// `_DAT_8007B850` - the packed per-frame pad mask.
    pub pad_mask: u32,
}

/// The writes one call performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatePickWrites {
    /// `scene[+0x2E]`, always `-1`.
    pub scene_slot_2e: i16,
    /// `scene[+0x40]` - the state that was current on entry.
    pub scene_saved_state: u16,
    /// `actor[+0x50]` - the state installed.
    pub actor_state: u16,
    /// `actor[+0x54]`, always `0`.
    pub actor_substate: u16,
}

/// Which state the gate selects. Split out from [`state_pick`] so the gate can
/// be asserted on its own.
pub fn picked_state(inputs: StatePickInputs) -> u16 {
    if inputs.script_resume_slot != 0 {
        return STATE_NORMAL;
    }
    if inputs.debug_mode == 0 {
        return STATE_NORMAL;
    }
    if inputs.pad_mask & PAD_DEBUG_BIT == 0 {
        return STATE_NORMAL;
    }
    STATE_DEBUG
}

/// Run the handler. `current_state` is the actor's `+0x50` on entry.
///
/// PORT: FUN_801f1f4c
// NOT WIRED: slot `7` of `PTR_FUN_801F33B4` is a return state, installed by
// the submode enter half `FUN_801F1278` at `0x801F140C` and parked in the
// cursor context's `+0x40` when the op-`0x49` sub-op names a screen
// (`0x801F1474..0x801F14AC`). `World::open_field_submode_screen` installs the
// screen's slot directly and never parks `7`, and no hand-back restores `+0x50`
// from `+0x40` (`HubGrid::stashed_state`), so nothing reaches this slot. The
// debug-mode word `_DAT_8007B98C` has no engine home either. See the module's
// `Not wired`.
pub fn state_pick(inputs: StatePickInputs, current_state: u16) -> StatePickWrites {
    StatePickWrites {
        scene_slot_2e: -1,
        scene_saved_state: current_state,
        actor_state: picked_state(inputs),
        actor_substate: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_script_forces_the_normal_state_even_in_debug() {
        let inputs = StatePickInputs {
            script_resume_slot: 0x800E_B297,
            debug_mode: 1,
            pad_mask: PAD_DEBUG_BIT,
        };
        assert_eq!(picked_state(inputs), STATE_NORMAL);
    }

    #[test]
    fn debug_arm_needs_both_the_mode_word_and_the_pad_bit() {
        let base = StatePickInputs::default();
        assert_eq!(picked_state(base), STATE_NORMAL);
        assert_eq!(
            picked_state(StatePickInputs {
                debug_mode: 1,
                ..base
            }),
            STATE_NORMAL
        );
        assert_eq!(
            picked_state(StatePickInputs {
                pad_mask: PAD_DEBUG_BIT,
                ..base
            }),
            STATE_NORMAL
        );
        assert_eq!(
            picked_state(StatePickInputs {
                debug_mode: 1,
                pad_mask: PAD_DEBUG_BIT,
                ..base
            }),
            STATE_DEBUG
        );
    }

    #[test]
    fn other_pad_bits_do_not_open_the_debug_arm() {
        let inputs = StatePickInputs {
            script_resume_slot: 0,
            debug_mode: 1,
            pad_mask: !PAD_DEBUG_BIT,
        };
        assert_eq!(picked_state(inputs), STATE_NORMAL);
    }

    #[test]
    fn both_arms_write_the_same_three_slots() {
        for (debug, pad) in [(0u32, 0u32), (1, PAD_DEBUG_BIT)] {
            let w = state_pick(
                StatePickInputs {
                    script_resume_slot: 0,
                    debug_mode: debug,
                    pad_mask: pad,
                },
                0x2A,
            );
            assert_eq!(w.scene_slot_2e, -1);
            assert_eq!(w.scene_saved_state, 0x2A);
            assert_eq!(w.actor_substate, 0);
        }
    }
}
