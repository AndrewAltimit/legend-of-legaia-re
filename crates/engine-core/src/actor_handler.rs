//! The actor's **`+0x0C` per-frame handler** - the identity retail's whole
//! actor model is keyed on, and the field the engine's [`Actor`] record was
//! missing.
//!
//! PORT: FUN_8003CF04, FUN_8003CF40
//!
//! ## Why a plain integer would not have been enough
//!
//! Retail actors are nodes on seven linked lists (`_DAT_8007C34C..0x36C`),
//! each carrying a function pointer at `+0x0C`. Three SCUS leaves are the
//! whole public API over it, and every one of them is an *identity* test:
//!
//! - the allocator `FUN_80020DE0` copies its descriptor's `+0x8` word into
//!   `actor[+0x0C]` (`docs/subsystems/asset-loader.md` has the descriptor
//!   layout `[+4 0xFFFF0000][+8 handler][+0xC flags]`);
//! - the frame dispatcher `FUN_8002519C` either `jalr`s `node[+0x0C]` or,
//!   when it equals `FUN_80021DF4`, runs the physics tick inline;
//! - the finder `FUN_8003CF04` and the retire sweep `FUN_8003CF40` both walk
//!   a list matching `node[+0x0C] == handler`, the finder additionally
//!   skipping nodes already carrying the kill bit `node[+0x10] & 8`.
//!
//! So the field has to do two jobs at once. It must compare equal to a raw
//! retail VA (that is what the scene-transition sweep
//! [`crate::field_actor_kernels::sweep_actor`], the submode open
//! [`crate::field_submode::open_submode`] and the field VM's own
//! find/retire opcodes are written against), and it must **select a ported
//! Rust kernel** so the dispatcher has something to run. [`ActorHandler`]
//! is that pair: a VA-preserving enum with named variants for the handlers
//! whose bodies are ported, `Retail(va)` for the rest, and
//! [`ActorHandler::kernel`] as the dispatch.
//!
//! ## Where the named variants come from
//!
//! Every VA below is pinned, not inferred, and most of them come from the
//! **spawn-descriptor words on the disc** rather than from a code reference -
//! which is the stronger evidence, because a descriptor's `+8` word is what the
//! allocator actually stamps. Reading `extracted/overlays/overlay_field_0897.bin`
//! at base `0x801CE818`:
//!
//! | descriptor VA | file | `+8` handler | who allocates from it |
//! |---|---|---|---|
//! | `0x801F2760` | `0x023F48` | `0x801D84D0` | `FUN_801D9C3C` (submode open) |
//! | `0x801F27EC` | `0x023FD4` | `0x801DA930` | `FUN_801DDE34` (fade family) |
//! | `0x801F2810` | `0x023FF8` | `0x801DBE9C` | `FUN_801DE478` (scene actor) |
//! | `0x801F2888` | `0x024070` | `0x801DDC20` | `FUN_801DE2B0` (colour tween) |
//! | `0x801F28A0` | `0x024088` | `0x80037174` | the `CC F8 80 N` narration op |
//!
//! `FUN_80021DF4` comes from a live capture instead - a
//! `battle_gimard_tail_fire` save reads it straight out of a running part
//! actor's `+0x0C` (`crates/mednafen/tests/firetail_movefx_liveness.rs`) - and
//! the four `0x801F****` widget handlers are the PROT 0900 descriptor words at
//! `0x801F8FE4/8FFC/9014/902C` ([`crate::screen_fx`]).
//!
//! REF: FUN_80020DE0 (installs the handler), FUN_8002519C (dispatches it)

use crate::field_actor_kernels::{MOVE_VM_HANDLER, RETIRED_HANDLERS};

/// `FUN_80021DF4` - the generic SCUS per-actor tick (drains the wait timer,
/// steps the move VM, emits the render/audio events). Identical to
/// [`MOVE_VM_HANDLER`]; the two names exist because the sweep talks about it
/// as "the move-VM handler" and the dispatcher as "the actor tick".
pub const VA_ACTOR_TICK: u32 = MOVE_VM_HANDLER;

/// `FUN_8002174C` - morph-weight apply pass (`see
/// docs/reference/functions/game-modes.md`). One of the three handler classes
/// the scene-transition sweep retires outright.
pub const VA_MORPH_WEIGHTS: u32 = 0x8002_174C;

/// `FUN_80037174` - the opening-narration crawl roller.
pub const VA_NARRATION_ROLLER: u32 = 0x8003_7174;

/// `FUN_801DA7F0` - the single-caption text-balloon handler (`4C E1`).
pub const VA_TEXT_BALLOON: u32 = 0x801D_A7F0;

/// `FUN_801DC0BC` - the cutscene camera mover.
pub const VA_CAMERA_MOVER: u32 = 0x801D_C0BC;

/// `FUN_801DDC20` - the per-actor colour tween.
pub const VA_COLOUR_TWEEN: u32 = 0x801D_DC20;

/// `FUN_801D84D0` - the op-`0x49` submode driver actor.
pub const VA_SUBMODE_DRIVER: u32 = crate::field_submode::SUBMODE_DRIVER_HANDLER;

/// `LAB_801DA930` - the **fade-family** actor handler. Spawn descriptor
/// `0x801F27EC` (field `0x023FD4`) carries it at `+8`, and `FUN_801DDE34` -
/// the `4C 90..92` fade ops' spawner - allocates from exactly that descriptor.
///
/// That closes a long-standing mislabel: the field VM's `4C 9F` and `4C 87`
/// "register callback" ops call `FUN_8003CF40(_DAT_8007C34C, LAB_801DA930)`,
/// and `FUN_8003CF40` **retires** rather than registers, so those ops cancel
/// the running fade. The MAN loader's first inlined sweep (`FUN_8003AEB0` at
/// `0x8003B3C8..0x8003B3F0`) is the same cancel on scene load.
pub const VA_FADE_FAMILY: u32 = 0x801D_A930;

/// `FUN_801DBE9C` - the handler on the fixed scene-actor template
/// `0x801F2810` that `FUN_801DE478` spawns from (field `0x023FF8`, word `+8`).
pub const VA_SCENE_ACTOR: u32 = 0x801D_BE9C;

/// `FUN_80037018` - the target of the MAN loader's *second* inlined retire
/// sweep (`0x8003B414..0x8003B43C`), immediately before it opens the submode.
pub const VA_MAN_LOAD_RETIRE_B: u32 = 0x8003_7018;

/// `FUN_801F7A9C` - PROT 0900 scripted-sprite widget.
pub const VA_SCREEN_SPRITE: u32 = 0x801F_7A9C;
/// `FUN_801F811C` - PROT 0900 screen-mask (iris / wipe) widget.
pub const VA_SCREEN_MASK: u32 = 0x801F_811C;
/// `FUN_801F849C` - PROT 0900 image-panel widget.
pub const VA_SCREEN_PANEL: u32 = 0x801F_849C;
/// `FUN_801F8A34` - PROT 0900 letterbox-band widget.
pub const VA_SCREEN_LETTERBOX: u32 = 0x801F_8A34;

/// One actor's `+0x0C` per-frame handler.
///
/// `Retail(va)` is not a fallback for "we did not bother": it is the faithful
/// representation of a handler whose body is not ported, and it still
/// compares, sweeps and retires exactly like a named one. Nothing about the
/// find / retire / transition-sweep semantics depends on having a kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ActorHandler {
    /// A null `+0x0C`. Retail never dispatches such a node; the engine uses
    /// it for actors that predate handler installation (actor-VM spawns,
    /// scene-bound NPC slots).
    #[default]
    None,
    /// [`VA_ACTOR_TICK`].
    ActorTick,
    /// [`VA_MORPH_WEIGHTS`].
    MorphWeights,
    /// [`VA_NARRATION_ROLLER`].
    NarrationRoller,
    /// [`VA_TEXT_BALLOON`].
    TextBalloon,
    /// [`VA_CAMERA_MOVER`].
    CameraMover,
    /// [`VA_COLOUR_TWEEN`].
    ColourTween,
    /// [`VA_SUBMODE_DRIVER`].
    SubmodeDriver,
    /// [`VA_FADE_FAMILY`].
    FadeFamily,
    /// [`VA_SCENE_ACTOR`].
    SceneActor,
    /// [`VA_SCREEN_SPRITE`].
    ScreenSprite,
    /// [`VA_SCREEN_MASK`].
    ScreenMask,
    /// [`VA_SCREEN_PANEL`].
    ScreenPanel,
    /// [`VA_SCREEN_LETTERBOX`].
    ScreenLetterbox,
    /// Any other retail `+0x0C` value, carried verbatim.
    Retail(u32),
}

/// What [`crate::world::World`] runs for an actor carrying a given handler.
///
/// This is the half that makes [`ActorHandler`] a dispatch rather than a
/// label. A handler with no ported body reports [`HandlerKernel::Unported`]
/// and the frame loop skips it - which is the honest engine behaviour, not a
/// silent no-op pretending to be a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerKernel {
    /// `engine-vm::actor_tick::tick_actor`, run inline by
    /// [`crate::world::World::tick_actor_physics`] - the same special case
    /// `FUN_8002519C` makes for this one handler.
    ActorTick,
    /// [`crate::field_actor_kernels::step_colour_tween`], run by
    /// [`crate::world::World::tick_handler_actors`].
    ColourTween,
    /// [`crate::cutscene_narration::CutsceneNarration`] - driven by the
    /// world's narration channel rather than by the actor loop, because the
    /// engine spawns the roller as host state (`World::cutscene_narration`)
    /// and not as a pool node.
    NarrationRoller,
    /// [`crate::text_balloon::TextBalloon`] - same shape as
    /// [`Self::NarrationRoller`]: ported, but hosted off the actor pool.
    TextBalloon,
    /// `engine-vm::camera_mover::CameraMover`, driven by
    /// [`crate::camera`].
    CameraMover,
    /// One of the four [`crate::screen_fx`] widget handlers.
    ScreenWidget,
    /// No ported body. The handler still participates in every identity
    /// test; it just has nothing to run.
    Unported,
}

impl HandlerKernel {
    /// `true` when [`crate::world::World::tick_handler_actors`] itself runs
    /// this kernel over the actor pool each frame. The other ported kernels
    /// are reached from their own host channels instead, which is why this
    /// is narrower than "is ported".
    pub fn runs_in_actor_loop(self) -> bool {
        matches!(self, HandlerKernel::ColourTween)
    }
}

impl ActorHandler {
    /// Classify a raw retail `+0x0C` word.
    pub fn from_va(va: u32) -> Self {
        match va {
            0 => ActorHandler::None,
            VA_ACTOR_TICK => ActorHandler::ActorTick,
            VA_MORPH_WEIGHTS => ActorHandler::MorphWeights,
            VA_NARRATION_ROLLER => ActorHandler::NarrationRoller,
            VA_TEXT_BALLOON => ActorHandler::TextBalloon,
            VA_CAMERA_MOVER => ActorHandler::CameraMover,
            VA_COLOUR_TWEEN => ActorHandler::ColourTween,
            VA_SUBMODE_DRIVER => ActorHandler::SubmodeDriver,
            VA_FADE_FAMILY => ActorHandler::FadeFamily,
            VA_SCENE_ACTOR => ActorHandler::SceneActor,
            VA_SCREEN_SPRITE => ActorHandler::ScreenSprite,
            VA_SCREEN_MASK => ActorHandler::ScreenMask,
            VA_SCREEN_PANEL => ActorHandler::ScreenPanel,
            VA_SCREEN_LETTERBOX => ActorHandler::ScreenLetterbox,
            other => ActorHandler::Retail(other),
        }
    }

    /// The retail `+0x0C` word this handler is. `0` for [`Self::None`].
    ///
    /// Round-trips with [`Self::from_va`] for every value, which is what lets
    /// disc-sourced and code-sourced handler references be compared without
    /// either side knowing which variant the other used.
    pub fn va(self) -> u32 {
        match self {
            ActorHandler::None => 0,
            ActorHandler::ActorTick => VA_ACTOR_TICK,
            ActorHandler::MorphWeights => VA_MORPH_WEIGHTS,
            ActorHandler::NarrationRoller => VA_NARRATION_ROLLER,
            ActorHandler::TextBalloon => VA_TEXT_BALLOON,
            ActorHandler::CameraMover => VA_CAMERA_MOVER,
            ActorHandler::ColourTween => VA_COLOUR_TWEEN,
            ActorHandler::SubmodeDriver => VA_SUBMODE_DRIVER,
            ActorHandler::FadeFamily => VA_FADE_FAMILY,
            ActorHandler::SceneActor => VA_SCENE_ACTOR,
            ActorHandler::ScreenSprite => VA_SCREEN_SPRITE,
            ActorHandler::ScreenMask => VA_SCREEN_MASK,
            ActorHandler::ScreenPanel => VA_SCREEN_PANEL,
            ActorHandler::ScreenLetterbox => VA_SCREEN_LETTERBOX,
            ActorHandler::Retail(va) => va,
        }
    }

    /// Which ported kernel this handler dispatches to.
    pub fn kernel(self) -> HandlerKernel {
        match self {
            ActorHandler::ActorTick => HandlerKernel::ActorTick,
            ActorHandler::ColourTween => HandlerKernel::ColourTween,
            ActorHandler::NarrationRoller => HandlerKernel::NarrationRoller,
            ActorHandler::TextBalloon => HandlerKernel::TextBalloon,
            ActorHandler::CameraMover => HandlerKernel::CameraMover,
            ActorHandler::ScreenSprite
            | ActorHandler::ScreenMask
            | ActorHandler::ScreenPanel
            | ActorHandler::ScreenLetterbox => HandlerKernel::ScreenWidget,
            ActorHandler::None
            | ActorHandler::MorphWeights
            | ActorHandler::SubmodeDriver
            | ActorHandler::FadeFamily
            | ActorHandler::SceneActor
            | ActorHandler::Retail(_) => HandlerKernel::Unported,
        }
    }

    /// `true` when the scene-transition sweep retires this handler class
    /// outright (`+0x10 |= 8`).
    ///
    /// The set is [`RETIRED_HANDLERS`], read through [`Self::from_va`] so the
    /// test is on the handler identity rather than on the enum shape - an
    /// `ActorHandler::Retail(0x80025000)` and a future named variant for the
    /// same VA both answer the same way.
    pub fn retired_by_scene_transition(self) -> bool {
        RETIRED_HANDLERS.contains(&self.va())
    }
}

/// The two handler classes the scene MAN loader retires before it opens the
/// submode, in retail order.
///
/// PORT: FUN_8003AEB0 (`0x8003B3C8..0x8003B3F0` and `0x8003B414..0x8003B43C`)
///
/// Both loops are `FUN_8003CF40` inlined: walk `_DAT_8007C34C` through the
/// `+0x00` next pointer, and `OR 8` into `+0x10` on every node whose `+0x0C`
/// equals the target. They run on **every** MAN load, which is what makes
/// them the load-bearing consumers of the handler field rather than a
/// curiosity - see [`crate::world::World::man_load_actor_reset`].
pub const MAN_LOAD_RETIRED_HANDLERS: [ActorHandler; 2] = [
    ActorHandler::FadeFamily,
    ActorHandler::Retail(VA_MAN_LOAD_RETIRE_B),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_round_trips_through_its_va() {
        let all = [
            ActorHandler::None,
            ActorHandler::ActorTick,
            ActorHandler::MorphWeights,
            ActorHandler::NarrationRoller,
            ActorHandler::TextBalloon,
            ActorHandler::CameraMover,
            ActorHandler::ColourTween,
            ActorHandler::SubmodeDriver,
            ActorHandler::FadeFamily,
            ActorHandler::SceneActor,
            ActorHandler::ScreenSprite,
            ActorHandler::ScreenMask,
            ActorHandler::ScreenPanel,
            ActorHandler::ScreenLetterbox,
            ActorHandler::Retail(0x8002_5000),
        ];
        for h in all {
            assert_eq!(ActorHandler::from_va(h.va()), h, "{h:?}");
        }
    }

    #[test]
    fn an_unnamed_va_is_still_a_full_citizen() {
        // The property the sweep depends on: a handler with no ported body
        // and no named variant must still test equal, and must still be
        // retired if it is in the sweep's set.
        let h = ActorHandler::from_va(0x8002_5000);
        assert_eq!(h, ActorHandler::Retail(0x8002_5000));
        assert_eq!(h.kernel(), HandlerKernel::Unported);
        assert!(h.retired_by_scene_transition());
    }

    #[test]
    fn the_sweeps_three_retire_targets_agree_with_the_enum() {
        // `RETIRED_HANDLERS` is the disassembly's list of three `+0x0C`
        // compares in FUN_801D7518; the enum must classify all three as
        // retired however each one happens to be spelled.
        for va in RETIRED_HANDLERS {
            assert!(
                ActorHandler::from_va(va).retired_by_scene_transition(),
                "{va:#010x}"
            );
        }
        // And the move-VM handler, which takes the sweep's LONG arm rather
        // than being retired, must not be in the set.
        assert!(!ActorHandler::ActorTick.retired_by_scene_transition());
    }

    #[test]
    fn the_colour_tween_is_one_of_the_retired_classes() {
        // The cross-link between the two ports in `field_actor_kernels`,
        // restated on the identity type: the sweep names the tween's entry.
        assert!(ActorHandler::ColourTween.retired_by_scene_transition());
        assert_eq!(ActorHandler::ColourTween.kernel(), HandlerKernel::ColourTween);
    }

    #[test]
    fn only_the_colour_tween_runs_inside_the_actor_loop() {
        // The other ported kernels are hosted off the pool (narration and
        // balloon are world channels, the camera mover is the camera's, the
        // widgets are `screen_fx`'s). Asserting this keeps a later variant
        // from quietly claiming an actor-loop tick it does not get.
        let in_loop: Vec<ActorHandler> = [
            ActorHandler::ActorTick,
            ActorHandler::ColourTween,
            ActorHandler::NarrationRoller,
            ActorHandler::TextBalloon,
            ActorHandler::CameraMover,
            ActorHandler::ScreenMask,
        ]
        .into_iter()
        .filter(|h| h.kernel().runs_in_actor_loop())
        .collect();
        assert_eq!(in_loop, vec![ActorHandler::ColourTween]);
    }

    #[test]
    fn the_man_load_retire_pair_is_not_the_transition_sweeps_set() {
        // Two different retire lists on the same field - the MAN loader's
        // pair and the transition sweep's three - and they are disjoint.
        for h in MAN_LOAD_RETIRED_HANDLERS {
            assert!(!h.retired_by_scene_transition(), "{h:?}");
        }
    }
}
