//! Disc-gated: the overworld player's locomotion clip and speed.
//!
//! A kingdom map is a mode-3 field-run scene: the world-map-walk overlay's
//! frame pump `FUN_801D1344` and pad controller `FUN_801D01B0` are
//! instruction-identical to the field overlay's, so the player writes its clip
//! base and binds it through the settle tail `FUN_801D1BA0` exactly as in a
//! town. Two overworld-specific inputs change what that controller does:
//!
//! - the player's `+0x72`, which each kingdom's entry script sets to `0xC00`
//!   (`CC F8 40 00 0C 00 00`) - the speed multiplier the pad step folds in
//!   (and the per-actor draw's render scale), `0xC00` on every retail
//!   overworld state against a town's `0x1000`;
//! - `_DAT_8007B6A8`, the per-scene MAN flag set on the kingdom maps, which
//!   forces the slow base step `5` (no run) and stores the scene-sentinel
//!   base `99` while a direction is held: the settle then binds clip
//!   `leader + 1` from the **scene** bank - record `leader` of the kingdom's
//!   own ANM bundle - instead of the party walk. Standing, the base is the
//!   idle `2` and the party-bank idle plays, as in a town.
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::character_pack;
use legaia_asset::player_anm::PlayerAnmBundle;
use legaia_engine_core::field_anim::FieldPlayerAnim;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::npc_catalog::scene_anm_bundle;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.locomotion.follow_terrain_height = true;
    host.enter_world_map_scene("map01").expect("enter map01");
    if let Some(ctrl) = host.world.world_map.ctrl.as_mut() {
        ctrl.view_mode = 0;
    }
    // The hosts install the leader's locomotion bank after the scene load.
    let bank = host
        .index
        .entry_bytes(character_pack::PROT_ENTRY_INDEX)
        .ok()
        .and_then(|b| character_pack::field_locomotion_anm(&b).ok())
        .expect("PROT 0874 locomotion bundle");
    let anim = FieldPlayerAnim::from_locomotion_bank(&bank, 0).expect("Vahn's bank");
    host.world.set_field_player_anim(Some(anim));
    for _ in 0..5 {
        host.world.set_pad(0);
        host.tick().expect("tick");
    }
    Some(host)
}

fn slot(host: &SceneHost) -> usize {
    host.world.player_actor_slot.expect("player") as usize
}

fn pos(host: &SceneHost) -> (i32, i32) {
    let ms = &host.world.actors[slot(host)].move_state;
    (i32::from(ms.world_x), i32::from(ms.world_z))
}

fn pose_key(host: &SceneHost) -> Vec<i32> {
    let a = &host.world.actors[slot(host)];
    let p = a
        .pose_frame
        .as_ref()
        .expect("the overworld tick poses the player");
    p.bone_outputs
        .iter()
        .flat_map(|(t, r)| t.iter().chain(r.iter()).copied())
        .map(i32::from)
        .collect()
}

/// One host frame: tick, then resolve the player's scene-bank pick against
/// the kingdom's own ANM bundle the way both play hosts do every sim tick
/// (`World::drain_field_anim_cues`).
fn frame(host: &mut SceneHost, pad: u16, scene_bundle: &PlayerAnmBundle) {
    host.world.set_pad(pad);
    host.tick().expect("tick");
    let _ = host
        .world
        .drain_field_anim_cues(Some(scene_bundle), None, |_| None);
}

#[test]
fn map01_entry_script_sets_the_overworld_speed_multiplier() {
    let Some(host) = open() else { return };
    let ms = &host.world.actors[slot(&host)].move_state;
    eprintln!("[ran] map01 player +0x72 = {:#x}", ms.field_72);
    assert_eq!(ms.field_72, 0x0C00, "map01 P1[0] `CC F8 40 00 0C 00 00`");
}

#[test]
fn map01_player_idles_on_the_party_bank_and_walks_on_the_kingdom_bank() {
    let Some(mut host) = open() else { return };
    // The kingdom's own ANM bundle - the one `_DAT_8007B888` points at on
    // every retail overworld state: records 0/1/2 are the 10-bone overworld
    // walk clips of Vahn / Noa / Gala.
    let scene_bundle =
        scene_anm_bundle(host.scene.as_ref().expect("scene")).expect("map01 scene ANM bundle");
    let rec0 = scene_bundle.record(0).expect("record 0");
    eprintln!(
        "[ran] map01 scene bundle: {} records, rec0 {} bones x {} frames",
        scene_bundle.record_count, rec0.bone_count, rec0.frame_count
    );
    assert_eq!((rec0.bone_count & 0xFF, rec0.frame_count), (10, 20));

    // Standing: base 2 strided into the party bank - the idle record.
    let mut idle_poses = HashSet::new();
    for _ in 0..40 {
        frame(&mut host, 0, &scene_bundle);
        idle_poses.insert(pose_key(&host));
    }
    {
        let anim = host.world.locomotion.player_anim.as_ref().unwrap();
        assert_eq!(
            anim.retail_slot(),
            Some(character_pack::LOCOMOTION_IDLE_SLOT)
        );
        assert_eq!(anim.scene_record(), None);
    }
    eprintln!(
        "[ran] idle distinct poses over 40 ticks: {}",
        idle_poses.len()
    );
    assert!(
        idle_poses.len() > 1,
        "the idle loop animates on the overworld"
    );

    // Walking (Up is open from the map01 seat - see map01_overworld_walk_disc).
    let before = pos(&host);
    let mut walk_poses = HashSet::new();
    for _ in 0..39 {
        frame(&mut host, PadButton::Up.mask(), &scene_bundle);
        walk_poses.insert(pose_key(&host));
        let anim = host.world.locomotion.player_anim.as_ref().unwrap();
        assert_eq!(anim.scene_record(), Some(0), "kingdom record `leader`");
        assert_eq!(host.world.locomotion.player_clip, 1, "clip id leader + 1");
        assert!(!host.world.locomotion.player_party_bank);
    }
    let after = pos(&host);
    let dist = (after.0 - before.0).abs() + (after.1 - before.1).abs();
    eprintln!(
        "[ran] walk: {} poses, {dist} units over 39 ticks",
        walk_poses.len()
    );
    assert!(walk_poses.len() > 1, "the overworld walk clip animates");
    assert!(
        walk_poses.is_disjoint(&idle_poses),
        "walking poses come from the kingdom clip, not the idle loop"
    );
    // ((5 * 0xC00) >> 12) * dt 3 = 9 a retail frame, rounded up to 10 by the
    // 2-unit stepper: the captured map01 tile crossings are 130 units every
    // 39 vsyncs, and the port's per-vsync tick lands the same displacement.
    assert_eq!(
        dist, 130,
        "the overworld slow step at the 0xC00 multiplier, retail's 130 / 39"
    );

    // No run on the overworld: the forced arm skips the run test, so a held
    // run button still walks at the slow step.
    let before = after;
    for _ in 0..39 {
        frame(
            &mut host,
            PadButton::Up.mask() | PadButton::Cross.mask(),
            &scene_bundle,
        );
    }
    let now = pos(&host);
    let dist = (now.0 - before.0).abs() + (now.1 - before.1).abs();
    assert_eq!(dist, 130, "no run on the overworld");

    // Release: back to the party-bank idle.
    frame(&mut host, 0, &scene_bundle);
    let anim = host.world.locomotion.player_anim.as_ref().unwrap();
    assert_eq!(
        anim.retail_slot(),
        Some(character_pack::LOCOMOTION_IDLE_SLOT)
    );
    assert_eq!(anim.scene_record(), None);
}
