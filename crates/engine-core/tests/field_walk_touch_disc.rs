//! Disc-gated: prop walk-touch events post from real MAN placement scripts
//! when the player's movement collides with the placement.
//!
//! Retail's locomotion (`FUN_801d01b0`) runs a touch dispatch on every
//! movement sub-step: a static-entity contact (the `FUN_801cfc40` probe's
//! result bit `4`, the mutual `+0x98` partner link) posts the touched
//! entity's event with **no button press** - `FUN_801d5b5c`, the same post
//! kernel the button-gated interact uses - and the dispatched script runs.
//! The engine mirrors the decoded script effects:
//!
//! - a genuine `0x3E` door-warp placement (`koin1`'s casino cabinets) posts
//!   the interact and **nothing else** - the decoded warp is the script's
//!   terminal instruction, past a coin compare and a confirm the contact does
//!   not clear, so entering the minigame on contact is not the retail effect;
//! - a cross-context `0x23` player-channel MOVE_TO placement (`cave01`'s
//!   throw-back guards) snaps the player to the decoded world position.
//!
//! Both post a `FieldEvent::FieldInteract` through the same
//! `trigger_field_interact` dispatch the interact probe uses, once per
//! contact. Structural assertions only (slots, coords, event shapes) - no
//! Sony bytes. Skip-passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_asset::man_section::parse as parse_man;
use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::WalkTouchEvent;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Build a `World` with `scene`'s MAN placements installed, or `None` = skip.
fn world_for_scene(scene_name: &str) -> Option<World> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let scene = Scene::load(&index, scene_name).expect("load scene");
    let man_bytes = scene
        .field_man_payload(&index)
        .expect("read MAN")
        .expect("scene has a MAN payload");
    let man_file = parse_man(&man_bytes).expect("parse MAN");
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.install_field_carriers_from_man(&man_file, &man_bytes);
    Some(world)
}

/// Walk the player from 200 units south (Z-) of `target` straight into it
/// (screen-up = world Z+ under the cold camera) and tick until either the
/// budget runs out or `done(world)` reports the touch landed.
fn walk_into(world: &mut World, target: (i16, i16), done: impl Fn(&World) -> bool) {
    world.install_field_player(0);
    world.actors[0].move_state.world_x = target.0;
    world.actors[0].move_state.world_z = target.1 - 200;
    world.set_pad(PadButton::Up.mask());
    for _ in 0..40 {
        let _ = world.tick();
        if done(world) {
            break;
        }
    }
    world.set_pad(0);
}

#[test]
fn koin1_cabinet_walk_touch_posts_the_interact_but_does_not_enter() {
    let Some(mut world) = world_for_scene("koin1") else {
        return;
    };

    // koin1 is the **casino**, and its several genuine `0x3E` door placements
    // are the venue's cabinets: the census (`minigame_entry_census_disc`)
    // decodes P1[54..56] as sub-id 3 (slot machine), P1[51..53] as sub-id 4
    // (Baka Fighter) and P1[9] as sub-id 5 (Muscle Dome). So the payload is a
    // mode-24 overlay selector, not a map id - see `minigame_entry`.
    let warps: Vec<(u8, (i16, i16), u8)> = world
        .field_walk_touch
        .iter()
        .filter_map(|(&slot, &(pos, event))| match event {
            WalkTouchEvent::Warp { sub_id } => Some((slot, pos, sub_id)),
            _ => None,
        })
        .collect();
    assert!(
        warps.len() >= 3,
        "koin1 derives several walk-touch door warps (got {})",
        warps.len()
    );
    let (slot, pos, sub_id) = warps[0];
    eprintln!("[koin1] walking into cabinet slot {slot} at {pos:?} (minigame sub_id {sub_id})");

    // Baseline: standing far from every portal posts no TOUCH. The
    // assertion is scoped to the walk-touch surface: channel stepping (when
    // an engaged window - a cutscene timeline or the liveliness opt-in -
    // has it running) may legitimately emit ambient field events.
    let _ = world.drain_field_events();
    for _ in 0..5 {
        let _ = world.tick();
    }
    assert!(world.pending_scene_transition.is_none());
    assert!(
        world
            .drain_field_events()
            .iter()
            .all(|e| !matches!(e, FieldEvent::FieldInteract { .. })),
        "no touch post while far from every portal"
    );

    // Walk into the cabinet's contact box. The touch posts once - retail's
    // `FUN_801d5b5c` script resume - and that is ALL it does. It must reach
    // neither warp channel:
    //
    // - `pending_scene_transition`, because resolving a minigame sub-id
    //   through the map-id resolver warped the player to an unrelated scene;
    // - `pending_minigame_warp`, because a cabinet's script is a coin compare
    //   into a confirm dialogue and only its taken arm reaches the `0x3E`.
    //   Running the *decoded* warp on contact skipped both gates, and koin1's
    //   casino NPCs stand inside their neighbouring cabinets' contact boxes,
    //   so walking up to an NPC to talk dropped the player into a minigame.
    let _ = sub_id;
    walk_into(&mut world, pos, |w| {
        w.pending_minigame_warp.is_some() || w.pending_scene_transition.is_some()
    });
    assert_eq!(
        world.pending_minigame_warp, None,
        "brushing a koin1 cabinet must not enter its minigame"
    );
    assert_eq!(
        world.pending_scene_transition, None,
        "a cabinet's sub-id must not reach the map-id resolver"
    );
    assert!(
        world.minigame_scene_backup.is_none(),
        "no mode-24 round trip is armed by a brush"
    );
    let events = world.drain_field_events();
    let posts = events
        .iter()
        .filter(|e| matches!(e, FieldEvent::FieldInteract { slot: s, .. } if *s == slot))
        .count();
    assert_eq!(posts, 1, "one touch post per contact (edge latch)");
}

#[test]
fn cave01_guard_walk_touch_teleports_the_player() {
    let Some(mut world) = world_for_scene("cave01") else {
        return;
    };

    // cave01's guard placements carry the cross-context player-channel
    // MOVE_TO (`0xA3 0xF8 …`): touching one throws the player back to a
    // fixed tile.
    type Throw = (u8, (i16, i16), (i16, i16));
    let throws: Vec<Throw> = world
        .field_walk_touch
        .iter()
        .filter_map(|(&slot, &(pos, event))| match event {
            WalkTouchEvent::PlayerMoveTo {
                world_x, world_z, ..
            } => Some((slot, pos, (world_x, world_z))),
            _ => None,
        })
        .collect();
    assert!(
        throws.len() >= 3,
        "cave01 derives several walk-touch player teleports (got {})",
        throws.len()
    );
    // Every guard throws to the same gathering tile - a structural pin of
    // the decoded target (all five retail guards converge on one spot).
    let target = throws[0].2;
    assert!(
        throws.iter().all(|&(_, _, t)| t == target),
        "all cave01 throw-backs target one world position"
    );
    let (slot, pos, _) = throws[0];
    eprintln!("[cave01] walking into guard slot {slot} at {pos:?} (throws to {target:?})");

    let _ = world.drain_field_events();
    walk_into(&mut world, pos, |w| {
        w.player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .is_some_and(|a| (a.move_state.world_x, a.move_state.world_z) == target)
    });
    let slot_idx = world.player_actor_slot.unwrap() as usize;
    let ms = &world.actors[slot_idx].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        target,
        "the walk-touch snapped the player to the guard's throw-back tile"
    );
    assert!(
        world.drain_field_events().iter().any(|e| matches!(
            e,
            FieldEvent::MoveTo {
                is_player: true,
                ..
            }
        )),
        "the teleport surfaced as a player MoveTo event"
    );
}
