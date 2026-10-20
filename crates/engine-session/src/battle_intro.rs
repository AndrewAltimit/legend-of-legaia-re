//! The field-to-battle intro emitter's host step, shared by both play hosts:
//! arm the emitter for the formation the encounter session is opening, step
//! it to the transition entity's clock, drop it when the transition ends.
//!
//! The simulation half is `World::tick_encounter` (which runs the transition
//! every frame the session sits in `Transition`) and `World::battle.intro`
//! (the entity whose `+0x1A` clock the styles ride). The emitter adopts that
//! clock rather than counting for itself, so a dropped or repeated
//! simulation tick cannot desynchronise the visuals from the handoff, and a
//! host may call this once per display frame or once per sim tick alike.
//!
//! What stays with each host is the capture: the native window re-renders its
//! presented frame offscreen and blits it into the style's texture pages; the
//! browser page reads its canvas back (bottom-up, hence
//! [`BattleIntro::with_flipped_capture`]) and composes the curtain's
//! intermediate on the CPU.

use legaia_engine_battle::encounter::EncounterPhase;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_render_kernels::battle_intro::BattleIntro;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// What one intro step left the host to do.
#[derive(Debug)]
pub enum BattleIntroStep {
    /// No transition is running; the emitter slot is empty. `dropped` is
    /// whether this step emptied it, so a host can retire what it built over
    /// the emitter (its captured page) once.
    Idle {
        /// The slot held an emitter before this step.
        dropped: bool,
    },
    /// The transition is armed but its entity has not ticked yet: the
    /// emitter (if any) is kept, and nothing draws this frame.
    Pending,
    /// This frame's primitives, the emitter stepped to the entity's clock
    /// and left in the slot.
    Frame {
        /// The primitives, in the OT buckets retail links them at.
        prims: Vec<ScreenPrim>,
        /// The transition entity's clock the emitter was stepped to.
        elapsed: i16,
        /// This step armed the emitter.
        armed: bool,
    },
}

/// Arm, advance or drop the intro emitter in `slot` so it tracks the
/// encounter session's `Transition` phase. `flipped_capture` marks a host
/// whose capture rows arrive bottom-up (a `gl.readPixels` readback).
pub fn step_battle_intro(
    host: &SceneHost,
    slot: &mut Option<BattleIntro>,
    flipped_capture: bool,
) -> BattleIntroStep {
    let session = host.world.encounters.session.as_ref();
    let Some(EncounterPhase::Transition { roll, .. }) = session.map(|s| s.phase()) else {
        return BattleIntroStep::Idle {
            dropped: slot.take().is_some(),
        };
    };
    let Some(entity) = host.world.battle.intro else {
        return BattleIntroStep::Pending;
    };
    let total = session.map_or(0, |s| i32::from(s.transition_frames));
    let armed = slot.is_none();
    let intro = slot.get_or_insert_with(|| {
        let intro = arm_battle_intro(host, roll.formation_id, total);
        if flipped_capture {
            intro.with_flipped_capture()
        } else {
            intro
        }
    });
    // Stepped to the entity's clock, one step per clock unit
    // (`BattleIntro::advance_to`).
    let prims = intro.advance_to(entity.elapsed).prims;
    BattleIntroStep::Frame {
        prims,
        elapsed: entity.elapsed,
        armed,
    }
}

/// Build the emitter for the battle about to open.
///
/// The style is not a choice the port makes: `select_intro_style` is a port
/// of the intro overlay's own init block, which keys on the battle flags
/// byte, the formation's first monster id and the current scene. All three
/// come out of `SceneHost::battle_intro_style_inputs`; the arming is
/// `BattleIntro::arm_for_battle` over the PROT 0979 tables (with their
/// disc-free fallbacks), the shade pack and the world's RNG seed.
pub fn arm_battle_intro(host: &SceneHost, formation_id: u16, total: i32) -> BattleIntro {
    let inputs = host.battle_intro_style_inputs(formation_id);
    let overlay = host
        .index
        .entry_bytes_extended(legaia_render_kernels::battle_intro::INTRO_OVERLAY_PROT)
        .ok();
    let shade = host
        .index
        .entry_bytes(legaia_actor_formats::field_char_textures::PROT_ENTRY_INDEX)
        .ok();
    let intro = BattleIntro::arm_for_battle(
        &inputs,
        total,
        overlay.as_deref(),
        shade.as_deref().map(Vec::as_slice),
        host.world.rng_state,
    );
    log::info!(
        "battle intro style {:?} (sub {}) for formation slot0 {:#04x}, battle flags {:#04x}",
        intro.style(),
        intro.sub_style(),
        inputs.formation_slot0,
        inputs.battle_flags
    );
    intro
}
