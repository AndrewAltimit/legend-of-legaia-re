//! Per-art attack-camera framing.
//!
//! PORT: FUN_801D71B8
//!
//! The per-frame routine that frames the camera on a party member's Arts swing.
//! It runs only while an Attack action is executing, resolves a **per-character,
//! per-art** script arm, and hands a rotation / translation / look-at triple to
//! the camera setter. One of the hottest bodies in the attack chain - see
//! [`docs/reference/functions/battle.md`](../../docs/reference/functions/battle.md).
//!
//! Transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801d71b8.txt` (1081
//! instructions).
//!
//! ## What this module ports
//!
//! * [`attack_camera_gate`] - the four-way entry test.
//! * [`CameraPose`] + [`CameraPose::seed`] - the local triple the arms mutate,
//!   including the negations that make the look-at the *inverse* of the actor's
//!   position and facing.
//! * [`character_arm`] / [`art_arm`] - the two dispatch layers: character id
//!   `1`/`2`/`3`, then art id through **that character's own** jump table.
//! * [`anim_push`] - the animation-frame-driven camera push the arms share,
//!   with its `0x60` bias, `<< 4` scale and `0x100` clamp.
//! * [`ArmTrack`] / [`arm_tracks`] / [`apply_track`] - which rows of the disc
//!   track table each arm folds into the pose, and into which component.
//!
//! ## Three jump tables, not one
//!
//! The earlier reading - "art id `0x1A..=0x2A` through a 17-slot jump table" -
//! was one character's table generalised to all three. Each character branch
//! bounds and indexes its own:
//!
//! | character id | bound | jump table | live arms |
//! |---|---|---|---|
//! | `1` | `0x11` (`0x801D72E0`) | `0x801CEA88` | 4 |
//! | `2` | `0x14` (`0x801D76C4`) | `0x801CEAD0` | 5 |
//! | `3` | `0x11` (`0x801D7B24`) | `0x801CEB20` | 4 |
//!
//! So character `2` reaches art ids up to `0x2D`, three past the range the
//! other two accept, and most slots in every table are the shared epilogue
//! `0x801D828C`. Thirteen arm bodies exist, not seventeen; several art ids
//! share one (`0x1E`/`0x2A` for characters `1` and `3`, `0x1D`/`0x2C` and
//! `0x1E`/`0x2D` for character `2`). See [`ART_JUMP_TABLES`].
//!
//! ## The track table
//!
//! Each arm reads one or more halfwords out of the disc table at
//! `0x801F4E10` - twenty rows of two, addressed `base + row*4 + cursor*2`
//! with `cursor = ctx[+0x26D]` - and adds them into the pose. Parser:
//! [`legaia_game_tables::battle_attack_camera_table`]. Every row is used and every
//! use is a fold into one pose component; [`ARM_TRACKS`] is the whole map,
//! taken site by site from the dump.
//!
//! ## Where it runs in the frame
//!
//! **Not** inside `FUN_801D5854` case `6`. The call site is that function's
//! shared tail, *after* case 6 has already built its own pose and handed it to
//! the tween builder (`0x801D7130`, `a3 = 0xC`):
//!
//! ```text
//! 801d7130  jal   0x801d829c        ; case 6's own tween (or case 9's, ...)
//! 801d7138  lui   v0,0x8008
//! 801d713c  lw    v1,0x46c0(v0)     ; _DAT_800846C0
//! 801d7144  beq   v1,0x2,0x801d7188 ; == 2 -> skip the per-art camera
//! 801d714c  lw    v0,-0x42dc(v0)    ; the battle context
//! 801d7154  lbu   v0,0x13(v0)       ; ctx[+0x13], the acting slot
//! 801d7164  lw    v0,0x0(v0)        ; DAT_801C9370[slot], the acting actor
//! 801d716c  lbu   v0,0x1dd(v0)      ; actor[+0x1DD], its target slot
//! 801d7174  sltiu v0,v0,0x8
//! 801d7178  beq   v0,zero,0x801d7188
//! 801d7180  jal   0x801d71b8        ; <- here
//! ```
//!
//! So the per-art camera is an **override**, not a fold: it seeds a fresh pose
//! from the actor ([`CameraPose::seed`]), adds its own arm's offsets, and calls
//! the *same* tween builder again with its own, much shorter duration (`1`, `3`
//! or `6` display frames against case 6's `0xC`). Whichever call ran last owns
//! the tween table that frame, and this one runs last. An art id with no arm
//! leaves case 6's framing standing. [`OUTER_GATE`] carries the two tail
//! conditions.
//!
//! The per-character arms that *are* inside case 6 (`0x801D5DAC` /
//! `0x801D5FC0` / `0x801D61E8`, dispatched at `0x801D5D50` and rejoining at
//! `0x801D645C`) sit between the base pose and the height floor and dispatch on
//! the **same** `actor[+0x1DB]` byte over a different band (`0x11..=0x18`,
//! bias `-0x11`, bound `8`). They are a separate family; this module ports the
//! `0x1A..=0x2D` one.
//!
//! ## What `actor[+0x1DB]` is
//!
//! Not a Tactical-Arts `ActionConstant`: it is the **latched battle-animation
//! id**. `FUN_8004AD80` copies `actor[+0x1DA]` (the staged anim id) into it
//! once per animation tick (`0x8004AEB0..0x8004AEB8`), so the camera arm that
//! runs is chosen by the clip that is playing. Ids `>= 0x10` are art-bank
//! records (`legaia_engine_vm::anim_vm::resolve_staged_anim`), so the arm band
//! `0x1A..=0x2D` is bank records `0x0A..=0x1D`.
//!
//! ## What `ctx[+0x26D]` is
//!
//! A **coin flip**, not a swing-phase counter. Its one writer is
//! `FUN_8004E13C` in `SCUS_942.54` (`0x8004E2DC`), which stores `rand() % 2`
//! on every call - and it is called from the anim commit `FUN_8004AD80`
//! (`0x8004BE28`) for every committed clip whose header byte `+0x87` is
//! non-zero, with that byte as its argument. Its other store,
//! `ctx[+0x6DA] = (rand() % 2) * 0x800 + 0x280` with `ctx[+0xD] = 0`, is
//! gated harder (`0x8004E25C..0x8004E280`: argument `2`, the previous
//! commit's `ctx[+0x243]` not `2`, and `ctx[+0x13] < 3`) - a party
//! attacker's first swing-clip commit; the shared battle camera applies that
//! seed on its own state edges (`battle_cam_script::BattleCamera::
//! observe_action_state`). Each of the twenty table rows therefore holds
//! **two alternative offsets** and retail picks one per commit, which is why
//! the same art frames from two visibly different angles on successive
//! swings. `FUN_801D5854` clears it to
//! `0` when the acting character id is `3` (`0x801D6A5C`).
//!
//! ## The three ramp counters
//!
//! `ctx[+0x26E]`, `ctx[+0x270]` and `ctx[+0x87C]` are advanced by
//! `FUN_801D5854`'s own prologue, on every call, by `8 * frame_step` - the two
//! byte ramps saturating at `0xC8` ([`AttackCamCtx::advance`],
//! `0x801D58F8..0x801D59B8`). Several arms re-zero `ctx[+0x26E]` through the
//! `ctx[+0x26F]` latch when the swing crosses an animation-frame threshold
//! ([`AttackCamCtx::latch_reset`]). All three are ported, so the arms carry
//! their literals and ramps rather than only their table folds.
//!
//! `ctx[+0x270]` is the odd one out: nothing in the per-art arms reads it.
//! Its only consumer is the case-8 **death re-frame** in the same function
//! (`0x801D6B50..0x801D6B98`, `battle_cam_script::apply_death_reframe`), which
//! is why the two byte ramps share an increment law but not a reset - the
//! death re-frame zeroes `ctx[+0x270]` itself the moment its own gate opens.
//!
//! Both byte ramps step through [`AttackCamCtx::ramp_step`], which models the
//! `sb` **before** the ceiling test: retail stores the sum into a byte and
//! then compares the *truncated* value against `0xC9`, so a frame step big
//! enough to carry past `0xFF` lands the ramp low rather than at the cap.

mod arm_tracks;
pub use arm_tracks::*;
mod pose;
pub use pose::*;
mod framing;
pub use framing::*;

/// Action category the gate demands (`actor[+0x1DE] == 3`, Attack).
pub const CATEGORY_ATTACK: u8 = 3;

/// Battle-phase byte the gate demands (`ctx[+6] == 0xFF`).
pub const PHASE_ACTIVE: u8 = 0xFF;

/// Party slots this routine frames (`ctx[+0x13] < 3`).
pub const PARTY_SLOTS: u8 = 3;

/// First art id with a camera arm (`actor[+0x1DB] - 0x1A`, table index `0`).
pub const FIRST_ART: u8 = 0x1A;

/// Base pitch/yaw magnitude the pose seeds two of its slots with (`0x400`).
pub const POSE_SEED_ANGLE: i16 = 0x400;

/// Animation-frame threshold below which the push is skipped (`slti a0, 0x61`).
pub const ANIM_PUSH_FLOOR: i16 = 0x61;

/// Bias subtracted from the animation frame before scaling (`addiu v0, -0x60`).
pub const ANIM_PUSH_BIAS: i16 = 0x60;

/// Ceiling on the scaled push (`sltiu a0, 0x101`, so the cap is `0x100`).
pub const ANIM_PUSH_CAP: i32 = 0x100;

#[cfg(test)]
mod tests;
