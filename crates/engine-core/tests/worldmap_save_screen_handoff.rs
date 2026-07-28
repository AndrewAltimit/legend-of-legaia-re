//! The fade/flash actor's save-screen hand-off, end to end through the world
//! tick.
//!
//! `FUN_801ED308`'s phase-1 arm ramps the screen to white, captures and clears
//! the tint triple, and then `jal`s `FUN_801D841C` at `0x801ED3DC` - thirteen
//! instructions that spawn descriptor `0x800706BC` into the system pool
//! `_DAT_8007C34C` and write `1` to `+0x5C` of the **returned** actor. That
//! descriptor's `+0x8` handler word is `0x80024190`, the in-field save/load
//! screen driver, and `+0x5C` is its save-vs-load discriminator - so the arm
//! hands the frame to the memory-card UI on the save side.
//!
//! The port dropped that call: the host saved and cleared the tint and
//! stopped, which left the actor parked at its hold phase with nothing able to
//! release it except a port-invented pad chord.
//!
//! What this covers is the join, over the real production path
//! `World::tick` -> `World::tick_world_map` -> `World::tick_world_map_panels`
//! -> `PanelActorHost::tick` -> `fade_flash_tick`:
//!
//! 1. the hand-off is produced, with retail's descriptor / pool / `+0x5C`;
//! 2. the actor parks on it, indefinitely, exactly as retail does while the
//!    menu overlay is resident;
//! 3. releasing it the way the menu overlay does - the counter write - walks
//!    the ramp back down and takes the terminal arm the counter selects;
//! 4. the release is **gated on the hand-off**, which is why the spawn is
//!    load-bearing rather than decorative.
//!
//! Synthetic world, no disc gate, no Sony bytes.

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_core::world_map::WorldMapController;
use legaia_engine_core::world_map_panel_host::{
    FLASH_RELEASE_COUNTER, FLASH_RELEASE_COUNTER_ALT, PanelActorKind, SaveScreenOutcome,
};
use legaia_engine_vm::world_map_panel_actors::{BRIGHTNESS_MAX, FADE_FLASH_HOLD_PHASE};

/// Descriptor `FUN_801D841C` passes to `FUN_80020DE0`.
const SAVE_SCREEN_DESCRIPTOR: u32 = 0x8007_06BC;
/// The pool discriminator, `_DAT_8007C34C`.
const SAVE_SCREEN_POOL: u32 = 0x8007_C34C;

/// A world sitting on the overworld with the debug band reachable - the same
/// gate `World::tick_world_map_panels` puts the whole panel screen behind.
fn overworld() -> World {
    let mut world = World::new();
    world.mode = SceneMode::WorldMap;
    world.world_map_ctrl = Some(WorldMapController {
        debug_enabled: true,
        ..WorldMapController::default()
    });
    world
}

/// Press `button` for one tick, then release it, so the chord reads as an edge.
fn tap(world: &mut World, button: PadButton) {
    world.input.set_pad(button.mask());
    let _ = world.tick();
    world.input.set_pad(0);
}

fn panels(world: &World) -> &legaia_engine_core::world_map_panel_host::PanelActorHost {
    &world.world_map_ctrl.as_ref().expect("controller").panels
}

fn panels_mut(world: &mut World) -> &mut legaia_engine_core::world_map_panel_host::PanelActorHost {
    &mut world.world_map_ctrl.as_mut().expect("controller").panels
}

/// Run ticks until `f` holds, or fail. Returns how many ticks it took.
fn run_until(world: &mut World, limit: usize, f: impl Fn(&World) -> bool) -> usize {
    for n in 1..=limit {
        let _ = world.tick();
        if f(world) {
            return n;
        }
    }
    panic!("condition not reached in {limit} ticks");
}

#[test]
fn the_ramp_hands_off_to_the_save_screen_with_retails_descriptor() {
    let mut world = overworld();
    tap(&mut world, PadButton::L1);
    assert_eq!(
        panels(&world).kind,
        Some(PanelActorKind::FadeFlash),
        "L1 installs the brightness fade/flash actor"
    );
    assert!(
        panels(&world).save_screen.is_none(),
        "nothing is handed off before the ramp saturates"
    );

    run_until(&mut world, 200, |w| panels(w).save_screen.is_some());

    let handoff = panels(&world).save_screen.expect("hand-off");
    assert_eq!(handoff.descriptor, SAVE_SCREEN_DESCRIPTOR);
    assert_eq!(handoff.pool, SAVE_SCREEN_POOL);
    assert!(
        handoff.is_save,
        "`sh 1,0x5c(v0)` is the save side; the fill-fade sibling leaves it zero"
    );

    // The tint capture is the same arm, so it has happened by now, and the
    // ramp is at or past the hand-off bias.
    assert!(panels(&world).brightness > 0);
}

#[test]
fn the_actor_parks_on_the_handoff_until_the_menu_side_answers() {
    let mut world = overworld();
    tap(&mut world, PadButton::L1);
    run_until(&mut world, 200, |w| {
        panels(w).phase == FADE_FLASH_HOLD_PHASE
    });

    assert_eq!(panels(&world).brightness, BRIGHTNESS_MAX);
    for _ in 0..300 {
        let _ = world.tick();
        assert_eq!(
            panels(&world).phase,
            FADE_FLASH_HOLD_PHASE,
            "the hold phase re-pins the level and waits; nothing self-releases it"
        );
        assert!(panels(&world).save_screen.is_some());
    }
}

#[test]
fn the_counter_the_menu_side_leaves_selects_the_terminal_arm() {
    for (outcome, counter, want_handler) in [
        (SaveScreenOutcome::HandlerA, FLASH_RELEASE_COUNTER, 0x29u16),
        (SaveScreenOutcome::HandlerB, FLASH_RELEASE_COUNTER_ALT, 0x2B),
    ] {
        let mut world = overworld();
        tap(&mut world, PadButton::L1);
        run_until(&mut world, 200, |w| {
            panels(w).phase == FADE_FLASH_HOLD_PHASE
        });

        assert!(panels_mut(&mut world).release_flash_with(outcome));
        assert_eq!(panels(&world).flash_counter, counter);
        assert!(
            panels(&world).save_screen.is_none(),
            "the hand-off closes when the menu side answers"
        );

        // Ramp back down, restore the tint, and take the arm `counter - 1`
        // selects. The actor is dropped on the exit.
        run_until(&mut world, 400, |w| panels(w).kind.is_none());
        // `ActorExit::apply` writes the arm's new id into `ctx[+0x50]` and
        // parks the old one in `scene[+0x40]`, so the host's own mirrors are
        // the record of which arm ran.
        assert_eq!(panels(&world).handler_id, want_handler);
        assert_eq!(panels(&world).scene_field_40, 0x1A);
        assert_eq!(panels(&world).scene_field_2e, -1);
    }
}

/// The non-vacuity guard, stated as behaviour rather than as a comment: with
/// no outstanding hand-off the release does nothing, because in retail nothing
/// but the menu overlay's save UI writes `_DAT_8007B43C`, and that overlay is
/// only resident because the spawn put its driver on the pool.
#[test]
fn without_the_handoff_there_is_nothing_to_release() {
    let mut world = overworld();
    tap(&mut world, PadButton::L1);
    run_until(&mut world, 200, |w| {
        panels(w).phase == FADE_FLASH_HOLD_PHASE
    });

    // Drop the hand-off, i.e. undo the spawn.
    panels_mut(&mut world).save_screen = None;

    assert!(
        !panels_mut(&mut world).release_flash(),
        "no hand-off, no release"
    );
    assert_eq!(panels(&world).flash_counter, 0);
    for _ in 0..200 {
        let _ = world.tick();
    }
    assert_eq!(
        panels(&world).phase,
        FADE_FLASH_HOLD_PHASE,
        "the actor is still parked"
    );
    assert!(panels(&world).kind.is_some());
}
