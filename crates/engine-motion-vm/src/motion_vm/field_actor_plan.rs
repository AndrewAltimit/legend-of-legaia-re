//! The field actor pass's per-actor plan: which of the height / motion / script steps run this frame.

/// How `FUN_8003BC08`'s height arm updates a field actor's **Y position**
/// `+0x16` this frame, selected from the flag word `+0x10`. Only reached when
/// the actor's lifetime halfword `+0x5C` is non-negative and `flags & 2` is
/// clear; otherwise `+0x16` is left untouched ([`FieldActorHeight::Hold`]).
///
/// This arm was documented here as a *facing* arm, and it is not one. Two
/// things fix it, both from the disassembly. `+0x14` / `+0x18` are the
/// actor's X / Z - `FUN_80019278` reads them at `0x8001929C` / `0x800192A0`,
/// shifts each right by six and indexes the per-scene collision grid with the
/// result, the same tiling `docs/subsystems/field-locomotion.md` pins from a
/// runtime watchpoint - so `+0x16` between them is Y, not a heading. And
/// `FUN_80019278` itself is the **bilinear ground-height sampler**: it reads
/// four corner nibbles out of the `.MAP` `+0x4000` region, looks each up in
/// the sixteen-rung scratchpad ramp at `0x1F80035C`, sums them and shifts
/// right by two (`0x800193DC..0x80019438`). It returns a floor elevation.
/// Every other citation of it in this repo already says so -
/// `legaia_scene_formats::field_objects`, `legaia_scene_formats::field_map`,
/// `World::sample_field_floor_height` - and this one line was the outlier.
///
/// So the three live arms are a Y **override**, a Y **snap to floor**, and a
/// Y **glide to floor**, in that priority. The engine's two field height
/// controllers are the second and third (`World::locomotion.follow_terrain_height` and
/// `World::locomotion.vertical_settle`); `World::locomotion.eased_mirror_y` is the
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldActorHeight {
    /// Leave the current Y standing (the `flags & 2` guard, or the
    /// `(flags & 0x20200) == 0 && ambient_gate == false` early-out).
    Hold,
    /// `flags & 0x20000000`: write `-(+0x8E)` into `+0x16` outright, skipping
    /// both floor arms below - the branch at `0x8003BC60` jumps straight to
    /// the shared store. The halfword is the inverted-Y mirror an eased move
    /// publishes (`FUN_801DD4C4` at `0x801DD6B8`), so a scripted move holds
    /// its own height against the terrain follow.
    Mirror(i16),
    /// Snap `+0x16` to the sampled ground height (`FUN_80019278`); taken when
    /// `flags & 0x2000` is clear.
    SnapToFloor,
    /// `flags & 0x2000`: glide toward the sampled ground height, at most
    /// `rate` per frame - feed the sample and the current `+0x16` to
    /// [`rotate_toward_clamped`], whose clamped-delta step applies unchanged.
    /// `rate = frame_step * 6`.
    GlideToFloor { rate: i32 },
}

/// Which of the per-actor motion routines `FUN_8003BC08` dispatches this
/// frame, after its facing arm. Every flag is `false` when the global motion
/// suppress bit (`_DAT_1F800394 & 0x400`) is set - retail skips the whole
/// dispatch block during that freeze.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldMotionDispatch {
    /// `flags & 0x100 && path_target_present` -> the spline / path arm
    /// `FUN_80039B7C`.
    pub run_spline: bool,
    /// `flags & 0x400` -> the pursue / patrol motion VM ported in this
    /// module ([`step`], retail `FUN_8003774C`).
    pub run_pursue: bool,
    /// `scene_guard_clear && scripted_present && !(flags & 8)` -> the
    /// scripted motion VM `FUN_80038158` (`0x8003BD5C..0x8003BD98`).
    /// `scene_guard_clear` is `*(u16 *)(_DAT_801C6EA4 + 8) == 0`;
    /// `scripted_present` is `+0x80 != 0`.
    pub run_scripted: bool,
    /// `lifetime > 0 || flags & 0x1000` -> the move-table consumer
    /// `FUN_800204F8`.
    pub run_move_table: bool,
}

/// The full per-frame plan `FUN_8003BC08` computes for one field actor -
/// written from the disassembly. The function itself also bumps a global
/// frame counter (`_DAT_801C6ED0`) and, when `lifetime >= 0`, runs a
/// pre-update (`FUN_801D79E8`); those are surfaced as [`Self::pre_update`].
///
/// Inputs are the actor fields and globals the retail body reads; the port
/// returns the decision so the host can invoke whichever sub-updates it has
/// wired without this function reaching into engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldActorPlan {
    /// `true` when `lifetime (+0x5C) >= 0`: retail runs the `FUN_801D79E8`
    /// pre-update and then the height arm. When `false`, neither runs and
    /// `height` is [`FieldActorHeight::Hold`].
    pub pre_update: bool,
    /// The height arm's outcome - what this frame writes to `+0x16`.
    pub height: FieldActorHeight,
    /// The motion dispatch that follows.
    pub dispatch: FieldMotionDispatch,
}

/// Inputs to [`field_actor_plan`] - the actor fields and engine globals
/// `FUN_8003BC08` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldActorInputs {
    /// `+0x5C` lifetime / active halfword.
    pub lifetime: i16,
    /// `+0x10` flag word.
    pub flags: u32,
    /// `+0x8E` - the inverted-Y mirror; the override target is its negation.
    pub field_8e: i16,
    /// `+0x90 != 0` - path / emitter target present.
    pub path_target_present: bool,
    /// `+0x80 != 0` - scripted-motion bytecode present.
    pub scripted_present: bool,
    /// `_DAT_8007B6A8 != 0` - the ambient-facing enable that lets a
    /// non-target actor still face-update.
    pub ambient_gate: bool,
    /// `_DAT_1F800393` - the frame-step scalar (vsyncs per game tick); the
    /// glide rate is this times six (`0x8003BCA8..0x8003BCC0`).
    pub frame_step: u8,
    /// `*(u16 *)(_DAT_801C6EA4 + 8) == 0` - the scene record's **bracket
    /// guard** is clear.
    ///
    /// This was read as "no modal window / dialog open", and no writer
    /// supports that. Every writer a sweep of the dump corpus finds (a store
    /// at `+8` through a register loaded from `0x801C6EA4`), re-read in the
    /// image's own bytes, is one of two brackets, each setting it to `1`
    /// before a call and back to `0` after: the scene-init
    /// bind sweep `FUN_8003AEB0` (`0x8003B73C` / `0x8003B928`), and the field
    /// VM around its script-context spawn calls (`0x801E2820` / `0x801E282C`
    /// around `jal 0x8003CF7C`, `0x801E2BBC` / `0x801E2BD8` around
    /// `jal 0x8003A1E4`, PROT 0897 at base `0x801CE818`). Nothing holds it
    /// across a frame boundary, so between frames the scripted VM's gate is
    /// open - a dialogue does not close it.
    pub scene_guard_clear: bool,
    /// `_DAT_1F800394 & 0x400 != 0` - global motion-suppress freeze.
    pub global_suppress: bool,
}

/// Port of `FUN_8003BC08` - the field-actor per-frame motion driver
/// (`ghidra/scripts/funcs/8003bc08.txt`). Turns one actor's flags + the
/// engine globals into a [`FieldActorPlan`]: which facing law to apply and
/// which of the four motion routines to run.
///
/// The two halves are independently gated. The height arm runs only for a
/// live actor (`lifetime >= 0`) that is not itself frozen (`flags & 2`
/// clear); the dispatch block runs unless the global suppress bit is set,
/// regardless of lifetime. Retail runs `FUN_801D79E8` before the height arm
/// and always increments a frame counter - see [`FieldActorPlan::pre_update`].
///
/// PORT: FUN_8003BC08
///
/// Wired as the gate on the ambient motion VM: `World::tick_field_npc_ambient`
/// (reached from `World::tick`'s field arm on both play hosts) builds one
/// plan per ambient channel off the channel's own `+0x10` word
/// (`AmbientMotion::actor_flags`) and the scratchpad word it carries
/// (`_DAT_1F800394`), and steps `FUN_80038158`'s op loop only when
/// [`FieldMotionDispatch::run_scripted`] says so. The height arm is applied
/// by `World::tick_field_npc_heights` (the snap is the floor sample
/// `World::field_npc_render_y` takes, the glide [`rotate_toward_clamped`]),
/// and the move-table arm's clip restart is `World::carry_npc_run_anim`'s
/// change test. The spline / pursue arms have no per-NPC consumer here:
/// field NPCs keep their pursue legs in typed per-slot maps rather than on an
/// actor record.
///
/// REF: FUN_80019278 (the bilinear ground-height sampler both floor arms
/// write; **not** a bearing-to-target - see [`FieldActorHeight`]),
/// FUN_80039B7C, FUN_80038158, FUN_800204F8
pub fn field_actor_plan(inp: FieldActorInputs) -> FieldActorPlan {
    let pre_update = inp.lifetime >= 0;

    let height = if !pre_update || inp.flags & 2 != 0 {
        FieldActorHeight::Hold
    } else if inp.flags & 0x2000_0000 != 0 {
        FieldActorHeight::Mirror(inp.field_8e.wrapping_neg())
    } else if inp.flags & 0x0002_0200 == 0 && !inp.ambient_gate {
        FieldActorHeight::Hold
    } else if inp.flags & 0x2000 == 0 {
        FieldActorHeight::SnapToFloor
    } else {
        FieldActorHeight::GlideToFloor {
            rate: i32::from(inp.frame_step) * 6,
        }
    };

    let dispatch = if inp.global_suppress {
        FieldMotionDispatch::default()
    } else {
        FieldMotionDispatch {
            run_spline: inp.flags & 0x100 != 0 && inp.path_target_present,
            run_pursue: inp.flags & 0x400 != 0,
            run_scripted: inp.scene_guard_clear && inp.scripted_present && inp.flags & 8 == 0,
            run_move_table: inp.lifetime > 0 || inp.flags & 0x1000 != 0,
        }
    };

    FieldActorPlan {
        pre_update,
        height,
        dispatch,
    }
}
