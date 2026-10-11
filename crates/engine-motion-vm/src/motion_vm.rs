//! Per-actor "third motion" VM, ported from `FUN_8003774C`
//! (SCUS_942.54). Distinct from the actor / sprite VM in `legaia_engine_vm`
//! and the move-table VM in `legaia_engine_vm::move_vm`:
//!
//! PORT: FUN_8003774C
//!
//! (This module's [`bearing_to_yaw`] is an `f32` approximation of the bearing
//! helper `FUN_80019B28` used only by the face-target ramp; the byte-faithful
//! LUT port of that function lives in
//! `legaia_engine_battle_vm::battle_action::bearing_12bit`.)
//!
//! - The actor VM (FUN_801D6628) handles sprite spawn / despawn, bytecode-driven.
//! - The move VM (FUN_80023070) drives Tactical Arts / battle-action animation.
//! - The motion VM here drives **per-actor pursue / patrol / face-target** logic
//!   used for NPC movement on the field, camera follow paths, and "face the
//!   speaker" cinematic posing during dialog.
//!
//! ## Bytecode layout
//!
//! Each script entry is 1 + N bytes:
//!
//! ```text
//!   +0  u8 op_byte         ; bit 0x7F = opcode, bit 0x80 = "select target"
//!   +1  u8 target_id       ; only present if bit 0x80 set in op_byte;
//!                          ;   special ids: 0xF8 (player), 0xFB (linked)
//!   +N  u8 operand[...]    ; opcode-specific operands
//! ```
//!
//! When the high bit is set (and the id differs from the running actor's own
//! `+0x50`), the VM resolves a target actor and applies the body **to that
//! actor** (`0x8003774C..0x80037858`). `0xF8` resolves to the **player**
//! (`_DAT_8007c364`), not "this actor"; `0xFB` walks the list at
//! `_DAT_8007c34c` for the first node whose handler word `+0xC` is
//! `0x801DA51C` (the world-map entity SM); any other id scans the list at
//! `_DAT_8007c354` for a node whose `+0x50` equals it (an earlier reading
//! said `+0x14`). An unresolved target ends the call returning 0.
//!
//! ## Opcodes implemented
//!
//! Dispatch is a 22-entry jump table at `0x80010EE0` indexed by
//! `(op & 0x7F) - 0x37`; every slot not listed below is the default arm.
//!
//! | byte | case body  | name             | semantics                                |
//! |------|------------|------------------|------------------------------------------|
//! | 0x37 | 0x8003789C | CompassWalkFast  | walk X/Z along one of 8 compass directions for a frame budget, rate `0x80` |
//! | 0x38 | 0x800379FC | RotateToAngle    | ramp yaw to a compass LUT entry over a frame budget |
//! | 0x41 | 0x8003789C | CompassWalkSlow  | the same arm at rate `0x40`              |
//! | 0x43 | 0x80037FF0 | Hold             | never completes: jumps to the epilogue with the done flag clear |
//! | 0x47 | 0x80037B84 | MoveTowardTarget | step actor XZ toward `(tx, tz)`, snapping facing per step-direction change |
//! | 0x4C | 0x80037DE0 | FaceTarget       | ramp yaw to the target's live bearing over a frame budget |
//! |      | 0x80037FEC | (default arm)    | terminate with `done=true`               |
//!
//! ## How this VM changes an actor's facing
//!
//! Three of the six opcodes write the actor's 12-bit heading (`+0x26`), and
//! they split into **two laws**:
//!
//! - **Snap.** `0x47` `MoveTowardTarget` quantises the step direction to the
//!   eight-point compass and writes it outright ([`walk_facing_yaw`]) - once
//!   per leg at the first moving frame, and again whenever the step's axis
//!   signs change (the dominant-axis → diagonal cut). A walking actor
//!   therefore never holds an in-between angle and holds one compass heading
//!   for a whole straight leg - retail has no walk-turn interpolation
//!   (runtime-pinned per-frame on the Mei dinner walk-on: every leg holds a
//!   single heading write for its whole run).
//! - **Ramp.** `0x38` and `0x4C` interpolate toward a target angle over an
//!   explicit frame budget carried in their own operands, stepping
//!   `arc * speed / frames_remaining` per tick and snapping to the exact
//!   target on the terminal frame ([`rotate_step`]). The per-tick write-back
//!   is **raw u16 wrapping, never normalised into `0..0xFFF`** - a
//!   wrap-crossing turn holds out-of-range headings (`0xFFxx` on a
//!   decreasing ramp through zero) until the terminal snap lands in range.
//!   `0x38`'s target is a compass LUT index; `0x4C`'s is the *live* bearing
//!   to another actor, so the ramp tracks a target that is itself moving.
//!
//! Which of the two an NPC is running is a property of the bytecode, not of
//! the engine - see `docs/subsystems/field-locomotion.md` for the priority
//! order across every facing source.
//!
//! ## Port boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The Ghidra decompilation at
//! `ghidra/scripts/funcs/8003774c.txt` is the *spec*. Tests use synthetic
//! bytecode.
//! REF: FUN_80019B28, FUN_80023070, FUN_801D6628

mod state;
pub use state::*;
mod heading;
pub use heading::*;
mod step;
pub use step::*;
mod field_actor_plan;
pub use field_actor_plan::*;

#[cfg(test)]
mod tests;
