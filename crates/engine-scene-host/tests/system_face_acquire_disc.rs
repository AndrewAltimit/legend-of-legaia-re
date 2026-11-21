//! Disc-gated: the scene system script's per-frame face snaps acquire their
//! target on every pass.
//!
//! `town0e` `P1[0]` runs `CC 1E 85 00 00 F8` (villager `0x1E` faces the
//! player) and `geremi` `P1[0]` runs `CC 35 85 00 00 37` (`0x35` faces actor
//! `0x37`) at the head of a loop that yields once a game frame. Each is a
//! FaceTarget acquire with a zero budget: the arm's guard refuses a target
//! whose halt is up outside the spawn bracket (`FUN_801DE840`
//! `0x801E2148..0x801E2190`), and retail clears the bit the acquire raised on
//! the target's own tick later in the same frame (`FUN_8003774C`,
//! `0x80038004`; the system node runs ahead of the actor list). So the loop
//! never refuses itself: these tests pin that the port's guard leaves every
//! pass acquiring, and that the snap still turns the target.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

/// The placement slot a target byte resolves to.
fn slot_of(host: &SceneHost, id: u8) -> u8 {
    let view = host.world.channel_view();
    let ci = legaia_engine_field::field_channels::resolve_target(view, id)
        .unwrap_or_else(|| panic!("id {id:#x} resolves"));
    u8::try_from(view[ci].placement_index).expect("a placement")
}

/// Enter `scene`, find the system script's acquire `CC <target> 85 00 00
/// <toward>`, then run `frames` world ticks, moving what the target faces
/// (the player, or placement `toward`) to the other side of it before each
/// one. Returns, over the game frames: how many start and end on the
/// acquire (refused), how many ran, and whether the target's heading moved off its
/// spawn heading (the snap landed).
fn run(scene: &str, target: u8, toward: u8, frames: u32) -> (u32, u32, bool) {
    let dir = extracted_dir().expect("checked by the caller");
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    host.enter_field_scene(scene, 0).expect("enter the scene");
    let at = host
        .world
        .field_bytecode
        .windows(6)
        .position(|b| b == [0xCC, target, 0x85, 0x00, 0x00, toward])
        .unwrap_or_else(|| panic!("{scene}: the system acquire"));
    let slot = slot_of(&host, target);
    let spawn = host.world.npcs.headings.get(&slot).copied();
    let &(tx, tz) = host
        .world
        .npcs
        .positions
        .get(&slot)
        .expect("the target stands");
    let (mut refused, mut ran, mut turned) = (0u32, 0u32, false);
    for f in 0..frames {
        let dx: i16 = if f % 2 == 0 { 0x200 } else { -0x200 };
        if toward == 0xF8 {
            let p = usize::from(host.world.player_actor_slot.expect("a player"));
            host.world.actors[p].move_state.world_x = tx + dx;
            host.world.actors[p].move_state.world_z = tz;
        } else {
            let s = slot_of(&host, toward);
            host.world.npcs.positions.insert(s, (tx + dx, tz));
        }
        let pc = host.world.field_pc;
        let _ = host.world.tick();
        // A game frame the script stepped on.
        if host.world.clock.script_frame_delta() == 0 {
            continue;
        }
        ran += 1;
        // Refused: the pass stood on the acquire and is still there.
        refused += u32::from(pc == at && host.world.field_pc == at);
        turned |= host.world.npcs.headings.get(&slot).copied() != spawn;
    }
    (refused, ran, turned)
}

#[test]
fn town0e_faces_its_villager_on_every_pass() {
    if extracted_dir().is_none() {
        return;
    }
    let (refused, ran, turned) = run("town0e", 0x1E, 0xF8, 40);
    assert!(ran > 10, "{ran}");
    assert_eq!(refused, 0, "the system loop refused its own acquire");
    assert!(turned, "the snap turned the villager");
}

#[test]
fn geremi_faces_its_guard_on_every_pass() {
    if extracted_dir().is_none() {
        return;
    }
    let (refused, ran, turned) = run("geremi", 0x35, 0x37, 40);
    assert!(ran > 10, "{ran}");
    assert_eq!(refused, 0, "the system loop refused its own acquire");
    assert!(turned, "the snap turned the guard");
}

/// The guard is live: with a 16-frame FaceTarget leg already turning
/// `town0e`'s villager (a talk's `CC 1E 85 10 00 F8`), the system pass parks
/// on its acquire while the leg is in flight, then acquires again once the
/// leg lands.
#[test]
fn town0e_waits_out_a_turn_already_in_flight() {
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    host.enter_field_scene("town0e", 0)
        .expect("enter the scene");
    for _ in 0..8 {
        let _ = host.world.tick();
    }
    let at = host
        .world
        .field_bytecode
        .windows(6)
        .position(|b| b == [0xCC, 0x1E, 0x85, 0x00, 0x00, 0xF8])
        .expect("the system acquire");
    let slot = slot_of(&host, 0x1E);
    let p = usize::from(host.world.player_actor_slot.expect("a player"));
    let &(tx, tz) = host
        .world
        .npcs
        .positions
        .get(&slot)
        .expect("the target stands");
    host.world.actors[p].move_state.world_x = tx - 0x300;
    host.world.actors[p].move_state.world_z = tz;
    let turn = [0xCC, 0x1E, 0x85, 0x10, 0x00, 0xF8];
    assert_eq!(host.world.run_placement_facing_op(&turn, 0), Some(6));
    let mut parked = Vec::new();
    for _ in 0..60 {
        let pc = host.world.field_pc;
        let _ = host.world.tick();
        if host.world.clock.script_frame_delta() != 0 {
            parked.push(pc == at && host.world.field_pc == at);
        }
    }
    // The pass that was mid-loop when the turn was armed may end short of
    // the acquire; from the next one on it parks until the leg lands.
    let first = parked
        .iter()
        .position(|&p| p)
        .expect("the pass waits for the turn");
    let resumed = first
        + parked[first..]
            .iter()
            .position(|&p| !p)
            .expect("the loop resumes");
    // A 16-unit budget at the scene's frame step spans several game frames.
    assert!(resumed - first >= 3, "{parked:?}");
    assert!(
        parked[resumed..].iter().all(|&p| !p),
        "the pass parked again after the turn landed: {parked:?}"
    );
}
