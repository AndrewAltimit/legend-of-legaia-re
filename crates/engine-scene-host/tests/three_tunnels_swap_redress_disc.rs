//! Disc-gated: a three-actor-talk leader swap spawns the talk's own record,
//! so `nilboa`'s Three Tunnels re-dress the map for each member's ravine.
//!
//! `FUN_801D27E0`'s swap ends with `FUN_8003BDE0(x, z, +0x72, 1)`
//! (`0x801D2C58..0x801D2C78`): `+0x72` is the `43 02` instruction's record
//! byte less the first two partition counts, which the spawner adds back, so
//! the spawn is the byte as a global index. `nilboa` P2[0] arms the talk with
//! `0x5F`, P2[17]: per leader flag `0x10` / `0x11` / `0x12` it raises the
//! ravine latch `0x472` / `0x473` / `0x474`, shows the ravine's name and
//! dresses the boulder room - the Fire Ravine re-seats the boulders
//! (`4C C3` on script ids `0x27` / `0x28`, placements 13 / 14), the Wind
//! Ravine parks them at `(0x7F, 0x7F)`. Without the spawn the dressing stayed
//! whatever the scene-entry run of P2[17] chose, so a scene entered under the
//! Wind Ravine leader kept the boulders off the map for every later swap and
//! their push (`0x457`) could never be reached.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use std::path::PathBuf;
use std::sync::Arc;

/// The boulders' talk placements (`P1[13]` / `P1[14]`).
const BOULDERS: [u8; 2] = [13, 14];

/// Their spawn positions: tiles `(49, 65)` and `(51, 65)`.
const BOULDER_SPAWN: [(i16, i16); 2] = [(6272, 8320), (6592, 8320)];

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file() && p.join("CDNAME.TXT").is_file())
}

fn boulders(host: &SceneHost) -> [Option<(i16, i16)>; 2] {
    BOULDERS.map(|s| host.world.npcs.positions.get(&s).copied())
}

/// Tick with Cross pressed every other frame, so text boxes page on.
fn tick_pressing(host: &mut SceneHost, frames: usize) {
    for t in 0..frames {
        host.world.set_pad(if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.tick();
    }
    host.world.set_pad(0);
    let _ = host.tick();
}

/// Hand control to the next member and let the swap and its record finish.
fn swap(host: &mut SceneHost) -> Option<u8> {
    for _ in 0..200 {
        host.world.request_talk_leader_switch();
        let _ = host.tick();
        if host
            .world
            .dialog
            .three_actor_talk
            .is_some_and(|t| t.swap.phase != 0)
        {
            break;
        }
    }
    tick_pressing(host, 300);
    host.world.party.party_leader_slot
}

#[test]
fn a_leader_swap_redresses_the_three_tunnels_for_the_incoming_ravine() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let index = Arc::new(
        legaia_engine_scene::scene::ProtIndex::open_extracted(&dir).expect("open extracted"),
    );
    let scene = legaia_engine_scene::scene::Scene::load(&index, "nilboa").expect("nilboa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("nilboa MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");

    let mut host = SceneHost::new(index);
    host.world.load_party(legaia_save::Party::zeroed(3));
    host.enter_field_scene("nilboa", 0).expect("enter nilboa");
    tick_pressing(&mut host, 30);

    // The split cutscene arms the talk (`43 02 30 31 32 51 04 5F`).
    assert!(host.world.install_gated_p2_record(&mf, &man, 0), "P2[0]");
    let mut armed = None;
    for t in 0..30_000 {
        tick_pressing(&mut host, 1);
        if armed.is_none() && host.world.dialog.three_actor_talk.is_some() {
            armed = Some(t);
        }
        if armed.is_some_and(|a| t > a + 600) && !host.world.cutscene_timeline_active() {
            break;
        }
    }
    let talk = host
        .world
        .dialog
        .three_actor_talk
        .expect("the split armed the talk");
    assert_eq!(talk.duration, 0x5F, "the talk's record byte");
    assert_eq!(host.world.party.party_leader_slot, Some(0));
    assert_eq!(boulders(&host), BOULDER_SPAWN.map(Some));

    // Member 1 takes over: the Wind Ravine, no boulders.
    assert_eq!(swap(&mut host), Some(1));
    assert!(
        host.world.system_flag_test(0x473),
        "P2[17]'s Wind Ravine latch"
    );
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    assert_eq!(boulders(&host), [Some((hide, hide)); 2]);

    // Round to member 0 again: the Fire Ravine re-seats them.
    assert_eq!(swap(&mut host), Some(2));
    assert!(
        host.world.system_flag_test(0x474),
        "P2[17]'s Thunder Ravine latch"
    );
    assert_eq!(swap(&mut host), Some(0));
    assert!(
        host.world.system_flag_test(0x472),
        "P2[17]'s Fire Ravine latch"
    );
    assert_eq!(boulders(&host), BOULDER_SPAWN.map(Some));
}

/// The swap hands the player object to the incoming member, so the body the
/// hosts draw has to follow: `FUN_801D27E0` writes the leader word
/// `_DAT_8007B8F8` / `0x80084597` and the collapsed list's entry
/// (`0x801D2AEC..0x801D2B08`) and stores nothing to the player's mesh. The
/// port's rig used to read the battle composition's head, which the swap
/// never touches, so every member walked the ravines in Vahn's body.
#[test]
fn a_leader_swap_puts_the_incoming_member_in_the_players_body() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let index = Arc::new(
        legaia_engine_scene::scene::ProtIndex::open_extracted(&dir).expect("open extracted"),
    );
    let scene = legaia_engine_scene::scene::Scene::load(&index, "nilboa").expect("nilboa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("nilboa MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");

    let mut host = SceneHost::new(index);
    host.world.load_party(legaia_save::Party::zeroed(3));
    host.enter_field_scene("nilboa", 0).expect("enter nilboa");
    tick_pressing(&mut host, 30);
    assert!(host.world.install_gated_p2_record(&mf, &man, 0), "P2[0]");
    let mut armed = None;
    for t in 0..30_000 {
        tick_pressing(&mut host, 1);
        if armed.is_none() && host.world.dialog.three_actor_talk.is_some() {
            armed = Some(t);
        }
        if armed.is_some_and(|a| t > a + 600) && !host.world.cutscene_timeline_active() {
            break;
        }
    }
    assert!(
        host.world.dialog.three_actor_talk.is_some(),
        "the talk armed"
    );
    // The hosts drain the signal every frame; settle it on the arm-time rig.
    host.world.take_player_rig_change();
    let body = |host: &SceneHost| host.player_rig_mesh().and_then(|m| m.party_slot);
    assert_eq!(body(&host), Some(0), "Vahn leads into the split");

    for want in [1u8, 2, 0] {
        assert_eq!(swap(&mut host), Some(want));
        assert_eq!(host.world.field_leader(), want);
        assert!(
            host.world.take_player_rig_change(),
            "the swap to {want} signals a rig rebuild"
        );
        assert_eq!(
            body(&host),
            Some(usize::from(want)),
            "the player's body is member {want}'s field form"
        );
    }
}
