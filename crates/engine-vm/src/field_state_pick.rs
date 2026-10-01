//! The field overlay's debug-gated actor state pick: slot `7` of the
//! subsystem actor's handler table at `0x801F33B4`, which pushes the actor's current
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
//! 34 instructions (`0x801F1F4C..0x801F1FD0`, read from
//! `overlay_field_0897_801f1f4c.txt`), no stack frame, `jr ra` at `0x801F1FCC`
//! with a second `jr ra` immediately after at `0x801F1FD4` and the next function's
//! `addiu sp, sp, -0x18` prologue at `0x801F1FDC`. Like its siblings it is
//! reached through a table rather than a `jal`: the word `0x801F1F4C` sits at
//! VA `0x801F33D0` in the same image, slot `7` of the 52-entry table at
//! `0x801F33B4` (slot `0` is the close tick `FUN_801F2134`) that the
//! dispatcher `FUN_801F159C` indexes by the actor's `+0x50`.
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
//! # Where the engine runs it
//!
//! Slot `7` is the subsystem actor's **default** handler. The menu button's
//! accept in the field overlay's pad controller (`FUN_801D01B0`,
//! `0x801D0318..0x801D0328`) spawns the actor, and its installer
//! `FUN_801F1278` stores `7` into `+0x50` at `0x801F140C`, after the same
//! `scene[+0x2E]` / `scene[+0x40]` pair this handler writes:
//!
//! ```text
//! 801f13f4  sh   v0,0x2e(v1)     ; scene[+0x2E] = -1
//! 801f1400  sh   v0,0x40(v1)     ; scene[+0x40] = actor[+0x50]  (outgoing)
//! 801f1404  addiu v0,zero,7
//! 801f140c  sh   v0,0x50(s4)     ; actor[+0x50] = 7
//! 801f141c  sh   zero,0x54(s4)
//! ```
//!
//! When `_DAT_8007B450` names a sub-op the `0x801F33A4` table gives a slot
//! for, the installer immediately overwrites that `7` with the table's slot
//! (`0x801F1474..0x801F14AC`); none of the table's fourteen entries is `7`.
//! On the menu-button path nothing overwrites it, so this handler runs once on
//! the actor's first dispatch and installs [`STATE_NORMAL`] - the pause-menu
//! session `FUN_801ED308` - or [`STATE_DEBUG`] on the debug shortcut.
//!
//! The engine opens its pause menu from the hosts' Start edge rather than
//! from an actor, behind the shared gate
//! `legaia_engine_core::world::World::field_menu_open_allowed`. That gate asks
//! `World::field_menu_button_state`, which runs [`state_pick`] with the
//! actor's default slot, and opens the menu only on [`STATE_NORMAL`]. The
//! debug-mode word is the overworld controller's `debug_enabled` flag, the
//! engine's home for `_DAT_8007B98C`; field scenes carry retail's zero.
//!
//! The op-`0x49` screens' own use of the table
//! (`legaia_engine_core::field_submode_screen`) installs the sub-op's slot
//! directly, which is the installer's second store; it never needs slot `7`.

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
///
/// Wired: `legaia_engine_core::world::World::field_menu_button_state`, asked
/// by `World::field_menu_open_allowed`, the Start-edge gate the native
/// play-window, `BootSession::tick` and the browser play page all open the
/// pause menu behind.
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
