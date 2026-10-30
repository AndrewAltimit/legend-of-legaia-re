//! Disc-gated: the Rebirth pack's tower intro (`packs/rebirth/cutscenes.toml`
//! `tower_intro`) played through the live scene host.
//!
//! Entering the Atrium for the first time queues its arrival cutscene; the
//! Warden's familiar, absent until its `present` gate holds, comes on and
//! walks down to the party; the lines run on the inline dialogue runner under
//! a confirm press; the cutscene ends with the player free, the familiar at
//! its waiting tile and the seen flag up. A second arrival plays nothing and
//! finds the familiar standing there; played again by hand, Start skips it.
//! Skips when `LEGAIA_DISC_BIN` is unset.

use std::sync::Arc;

use legaia_engine_scene::expansion_map::tile_centre_world;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_expansion_format::ExpansionPack;
use legaia_expansion_format::cutscene::seen_flag;
use legaia_expansion_format::map::npc_slot;

const MAP: &str = "rebirth_tower_1f";
const HIDE: i16 = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;

fn host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    };
    let mut host = SceneHost::open_disc(disc).expect("open the disc");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
    let pack = ExpansionPack::from_dir(std::path::Path::new(dir)).expect("Rebirth parses");
    host.activate_expansion_pack(Arc::new(pack))
        .expect("Rebirth activates");
    host.world.toggles.use_vm_dialogue = true;
    Some(host)
}

fn tick(host: &mut SceneHost, pad: u16) {
    host.world.set_pad(pad);
    host.tick().expect("tick");
}

fn player(host: &SceneHost) -> (i16, i16) {
    let m = &host.world.actors[0].move_state;
    (m.world_x, m.world_z)
}

#[test]
fn the_tower_intro_plays_once_on_arrival_and_skips_once_seen() {
    let Some(mut host) = host() else {
        return;
    };
    host.enter_expansion_map(MAP, "entrance")
        .expect("enter the Atrium");
    let def = host.world.expansion.map.as_ref().unwrap().def.clone();
    let familiar = def
        .npcs
        .iter()
        .position(|n| n.id == "warden_familiar")
        .and_then(npc_slot)
        .expect("the familiar is a map NPC");
    assert_eq!(
        host.world.npcs.positions.get(&familiar),
        Some(&(HIDE, HIDE)),
        "absent before the intro"
    );
    assert!(
        host.world
            .expansion
            .cutscene
            .queued
            .iter()
            .any(|c| c == "tower_intro"),
        "the arrival queued the intro"
    );

    // Play it: confirm presses page the lines; the pad does not walk anyone.
    let start = player(&host);
    let mut frames = 0;
    let mut saw_on = false;
    let mut walked = false;
    while frames < 6000 {
        let pad = if frames % 3 == 0 {
            PadButton::Cross.mask()
        } else {
            PadButton::Up.mask()
        };
        tick(&mut host, pad);
        frames += 1;
        let at = host.world.npcs.positions.get(&familiar).copied();
        saw_on |= at.is_some_and(|p| p != (HIDE, HIDE));
        walked |= host
            .world
            .expansion
            .cutscene
            .walks
            .contains_key("warden_familiar");
        if frames > 2 && !host.world.expansion_cutscene_active() {
            break;
        }
        assert!(
            !host.world.expansion_cutscene_active() || host.world.script_context_engages_player(),
            "held throughout"
        );
    }
    eprintln!("[{MAP}] the intro ran {frames} frames");
    assert!(!host.world.expansion_cutscene_active(), "the intro ended");
    assert!(saw_on && walked, "the familiar came on and walked");
    assert_eq!(player(&host), start, "the player stood where it arrived");
    let flags = &host.world.expansion.quests.progress.flags;
    assert!(flags.is_set(&seen_flag("tower_intro")));
    // Let the familiar's last, unwaited leg land.
    for _ in 0..240 {
        tick(&mut host, 0);
    }
    let wait_at = tile_centre_world(&def, 4, 8);
    assert_eq!(host.world.npcs.positions.get(&familiar), Some(&wait_at));

    // A second arrival: no intro, and the familiar is present on its tile.
    host.enter_expansion_map(MAP, "entrance").expect("re-enter");
    assert!(host.world.expansion.cutscene.queued.is_empty());
    for _ in 0..30 {
        tick(&mut host, 0);
    }
    assert!(!host.world.expansion_cutscene_active());
    assert_eq!(host.world.npcs.positions.get(&familiar), Some(&wait_at));

    // Played again by hand, it is skippable: Start lands the rest at once.
    assert!(host.world.expansion_play_cutscene("tower_intro"));
    for _ in 0..5 {
        tick(&mut host, 0);
    }
    assert!(host.world.expansion_cutscene_active());
    tick(&mut host, PadButton::Start.mask());
    tick(&mut host, 0);
    assert!(!host.world.expansion_cutscene_active(), "skipped");
    assert!(!host.world.script_context_engages_player());
}
