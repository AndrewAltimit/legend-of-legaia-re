//! The field overlay's **scene-transition teardown sweep** (`FUN_801D7518`):
//! the per-actor pass that retires, re-stamps and re-buffers the actor lists
//! on a warp entry.
//!
//! PORT: FUN_801D7518
//!
//! Its second retire test compares the actor's per-frame handler against
//! `0x801DDC20`, the entry of the colour tween in
//! [`crate::actor_colour_tween`]; decoding either one predicts a field of the
//! other.
//!
//! Provenance: `overlay_cutscene_dialogue_801d7518.txt`, cross-checked against
//! the `overlay_cutscene_mapview_*` and `overlay_world_map_*` field captures
//! (identical sizes).
//!
//! [`Actor::handler`] carries the `+0x0C` identity the sweep keys on
//! ([`crate::actor_handler::ActorHandler`]). [`sweep_actor`] is **live** -
//! `SceneHost::load_scene` runs `World::scene_transition_actor_sweep` over the
//! pool on every scene-to-scene change.
//!
//! [`Actor::handler`]: legaia_engine_core::world::Actor::handler
//!
//! REF: FUN_801D6704 (calls the sweep once per actor list on a warp entry),
//! FUN_80017888 (buffer alloc)

/// Actor flag bit meaning "retire me at the end of this frame".
pub const ACTOR_FLAG_YIELD: u32 = 0x8;

/// Actor flag bit the sweep stamps on **every** actor it visits - the
/// scene-transition marker.
pub const ACTOR_FLAG_TRANSITION: u32 = 0x1_0000;

/// Actor flag bit meaning "this actor owns a `0x9C`-byte side buffer at
/// `+0x44`", which the sweep reallocates and re-seeds.
pub const ACTOR_FLAG_HAS_SIDE_BUFFER: u32 = 0x800;

/// Size of that side buffer.
pub const ACTOR_SIDE_BUFFER_BYTES: usize = 0x9C;

/// Render mode (low nibble of `+0x56`) whose accumulator the sweep reseeds -
/// the ocean CLUT-walk emitter.
pub const RENDER_MODE_CLUT_WALK: u16 = 0xB;

/// Accumulator value written to a CLUT-walk actor's `+0x68`. It is deliberately
/// at or above any hold value, so the first frame after the transition fires a
/// copy immediately.
pub const CLUT_WALK_ACC_SEED: u16 = 0x64;

/// Per-frame handler addresses whose actors the sweep retires outright.
///
/// PORT: FUN_801D7518 (`0x801d7550..0x801d75b4`).
///
/// Three equality tests against the actor's `+0x0C` handler pointer, each
/// setting [`ACTOR_FLAG_YIELD`]. The second is the colour tween in this same
/// module ([`step_colour_tween`]).
///
/// These are raw retail code addresses; the engine compares against them
/// through [`crate::actor_handler::ActorHandler::retired_by_scene_transition`],
/// which reads this array. They stay VAs rather than becoming enum variants
/// because a VA is what the disassembly compares - the enum is a view over
/// this list, not a replacement for it.
pub const RETIRED_HANDLERS: [u32; 3] = [0x8002_5000, 0x801D_DC20, 0x8002_174C];

/// The move-VM actor handler, whose actors take the sweep's long arm instead of
/// being retired.
pub const MOVE_VM_HANDLER: u32 = 0x8002_1DF4;

/// One actor as the teardown sweep sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepActor {
    /// `+0x0C` - per-frame handler address.
    pub handler: u32,
    /// `+0x10` - flag word.
    pub flags: u32,
    /// `+0x56` - render mode word (only the low nibble is read).
    pub render_mode: u16,
}

/// What the sweep decides for one actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepDecision {
    /// The actor's `+0x10` after the sweep's ORs.
    pub flags: u32,
    /// `true` when the actor is retired this frame.
    pub retired: bool,
    /// `true` when the sweep must allocate the actor a fresh
    /// [`ACTOR_SIDE_BUFFER_BYTES`] buffer at `+0x44` and seed it
    /// (`+0x94`/`+0x96`/`+0x98` cleared, `+0x9A` set to `-1`).
    pub realloc_side_buffer: bool,
    /// `Some(seed)` when the actor's `+0x68` accumulator is reseeded.
    pub clut_walk_seed: Option<u16>,
    /// `true` when the actor takes the move-VM arm, which rebuilds its
    /// geometry/keyframe buffers. That arm's buffer plumbing is not modelled
    /// here - see the note on [`sweep_actor`].
    pub move_vm_arm: bool,
}

/// Decide the teardown sweep's effect on one actor.
///
/// PORT: FUN_801D7518 (`0x801d7544..0x801d77c0`, one loop iteration).
///
/// Retail runs this over a whole actor list, and the field initialiser calls it
/// **once per list** - seven times - on a warp entry (`_DAT_8007B8B8 == 2`),
/// which is what makes a warp structurally different from a cold entry (see
/// `legaia_engine_system::mode_entry_init::FieldEntryMode`).
///
/// The order the flags land in matters: the three handler tests OR
/// [`ACTOR_FLAG_YIELD`] in, then [`ACTOR_FLAG_TRANSITION`] is stamped
/// unconditionally, and only **then** is [`ACTOR_FLAG_HAS_SIDE_BUFFER`] tested -
/// against the *updated* word. Since none of the earlier ORs touch bit `0x800`
/// the outcome is the same either way, but the read is of the updated word and
/// a future edit to those ORs would change behaviour.
///
/// The `MOVE_VM_HANDLER` arm's inner work (a keyframe-buffer allocation gated on
/// `+0x5A == 3` and `+0xA4 > 0x10`, and a six-column vertex copy gated on
/// `+0x5A == 6`) is reported as [`SweepDecision::move_vm_arm`] rather than
/// modelled: both are raw buffer plumbing against actor-local scratch the
/// engine does not allocate.
///
/// Live: `SceneHost::load_scene` calls
/// [`World::scene_transition_actor_sweep`] whenever a scene is already loaded
/// (the engine's form of retail's `_DAT_8007B8B8 == 2` warp gate), which runs
/// this once per pool slot. Host roots that reach `load_scene`:
/// `SceneHost::enter_field_scene` (→ `BootSession`, `legaia-engine` `run` /
/// `play-window`) and the door-warp path in `SceneHost::tick`.
///
/// [`World::scene_transition_actor_sweep`]: legaia_engine_core::world::World::scene_transition_actor_sweep
pub fn sweep_actor(actor: SweepActor) -> SweepDecision {
    let retired = RETIRED_HANDLERS.contains(&actor.handler);
    let mut flags = actor.flags;
    if retired {
        flags |= ACTOR_FLAG_YIELD;
    }
    flags |= ACTOR_FLAG_TRANSITION;
    SweepDecision {
        flags,
        retired,
        realloc_side_buffer: flags & ACTOR_FLAG_HAS_SIDE_BUFFER != 0,
        clut_walk_seed: (actor.render_mode & 0xF == RENDER_MODE_CLUT_WALK)
            .then_some(CLUT_WALK_ACC_SEED),
        move_vm_arm: actor.handler == MOVE_VM_HANDLER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(handler: u32) -> SweepActor {
        SweepActor {
            handler,
            flags: 0,
            render_mode: 0,
        }
    }

    #[test]
    fn the_sweep_retires_exactly_the_three_named_handlers() {
        for h in RETIRED_HANDLERS {
            let d = sweep_actor(actor(h));
            assert!(d.retired, "{h:#x} should retire");
            assert_eq!(d.flags & ACTOR_FLAG_YIELD, ACTOR_FLAG_YIELD);
        }
        let d = sweep_actor(actor(0x8002_9999));
        assert!(!d.retired);
        assert_eq!(d.flags & ACTOR_FLAG_YIELD, 0);
    }

    #[test]
    fn the_colour_tween_is_one_of_the_retired_handlers() {
        // The cross-link that corroborates both ports: the sweep names the
        // tween's own entry address.
        assert!(RETIRED_HANDLERS.contains(&0x801D_DC20));
    }

    #[test]
    fn every_visited_actor_takes_the_transition_stamp() {
        for h in [0x8002_5000, 0x8002_9999, MOVE_VM_HANDLER] {
            let d = sweep_actor(actor(h));
            assert_eq!(d.flags & ACTOR_FLAG_TRANSITION, ACTOR_FLAG_TRANSITION);
        }
    }

    #[test]
    fn the_side_buffer_realloc_follows_the_actors_own_bit() {
        let mut a = actor(0);
        a.flags = ACTOR_FLAG_HAS_SIDE_BUFFER;
        assert!(sweep_actor(a).realloc_side_buffer);
        a.flags = 0;
        assert!(!sweep_actor(a).realloc_side_buffer);
    }

    #[test]
    fn only_render_mode_b_reseeds_the_clut_walk_accumulator() {
        let mut a = actor(0);
        a.render_mode = RENDER_MODE_CLUT_WALK;
        assert_eq!(sweep_actor(a).clut_walk_seed, Some(CLUT_WALK_ACC_SEED));
        // The nibble is masked, so the high bits of +0x56 do not matter.
        a.render_mode = 0x5B;
        assert_eq!(sweep_actor(a).clut_walk_seed, Some(CLUT_WALK_ACC_SEED));
        a.render_mode = 5;
        assert_eq!(sweep_actor(a).clut_walk_seed, None);
    }
}
