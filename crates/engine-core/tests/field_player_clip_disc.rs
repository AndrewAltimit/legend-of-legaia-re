//! Disc-gated: the field player's locomotion clip is the one retail's clip
//! base names - idle, walk, **run**, and a warp's walk-in-place - picked by
//! the settle tail (`FUN_801D1BA0` at `0x801D1D88..0x801D1EAC`) from the base
//! the pad step writes (`FUN_801D01B0` at `0x801D0424..0x801D04A4`).
//!
//! Driven through `SceneHost::tick` on `town01` with the player's clips built
//! the way both play hosts build them (`FieldPlayerAnim::from_locomotion_bank`
//! over PROT 0874 section 1). Structural assertions only (bank slots, clip
//! frame counts) - no Sony bytes. Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::field_anim::FieldPlayerAnim;
use legaia_engine_core::input::PadButton;
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

fn open_town01() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("town01", 0).expect("enter town01");
    for _ in 0..3 {
        host.tick().expect("tick");
    }
    let bytes = host
        .index
        .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
        .expect("PROT 0874");
    let bundle = legaia_asset::character_pack::field_locomotion_anm(&bytes).expect("section 1");
    let anim = FieldPlayerAnim::from_locomotion_bank(&bundle, 0).expect("Vahn's bank");
    for slot in 0..7 {
        assert!(anim.has_bank_slot(slot), "bank slot {slot} decodes");
    }
    host.world.set_field_player_anim(Some(anim));
    // Seat the player on open ground in the village square, clear of any
    // walk-on tile, so a few frames of walking cross no trigger.
    let s = host.world.player_actor_slot.expect("player") as usize;
    host.world.actors[s].move_state.world_x = 2624;
    host.world.actors[s].move_state.world_z = 2624;
    host.world.actors[s].move_state.flags &= !0x0008_0000;
    Some(host)
}

fn drive(host: &mut SceneHost, pad: u16, frames: u32) {
    for _ in 0..frames {
        host.world.set_pad(pad);
        host.tick().expect("tick");
    }
}

fn slot(host: &SceneHost) -> Option<usize> {
    host.world
        .locomotion
        .player_anim
        .as_ref()
        .and_then(|a| a.retail_slot())
}

#[test]
fn town01_pad_picks_idle_walk_and_run_from_the_leaders_bank() {
    let Some(mut host) = open_town01() else {
        return;
    };
    if host.world.party.scene_save_allowed {
        // `_DAT_8007B6A8` routes a moving player to the scene-bank sentinel;
        // this test is about the party bank.
        panic!("town01 unexpectedly carries the MAN[0x01] scene flag");
    }
    drive(&mut host, 0, 4);
    assert_eq!(host.world.locomotion.clip_base, 2, "idle base");
    assert_eq!(slot(&host), Some(1), "idle = bank slot 1");

    drive(&mut host, PadButton::Up.mask(), 4);
    assert_eq!(host.world.locomotion.clip_base, 1, "walk base");
    assert_eq!(slot(&host), Some(0), "walk = bank slot 0");

    // R1 is in the retail run mask `0x48`; with the Field Move option on
    // Walk it inverts to run.
    drive(&mut host, PadButton::Up.mask() | PadButton::R1.mask(), 4);
    assert_eq!(host.world.locomotion.clip_base, 3, "run base");
    assert_eq!(slot(&host), Some(2), "run = bank slot 2");
    assert_eq!(
        host.world.locomotion.player_clip, 3,
        "Vahn's run clip id = base + leader * 7"
    );
    let frames = host
        .world
        .locomotion
        .player_anim
        .as_ref()
        .map(|a| a.active_frame_count())
        .unwrap();
    assert!(frames > 0, "the run clip plays");

    drive(&mut host, 0, 4);
    assert_eq!(slot(&host), Some(1), "back to idle on release");
}

/// Vahn's-house interior (tile `(97,10)`) and its kind-0 exit one tile toward
/// the door at `(97,9)`, whose record lands on the doorstep at half-tile
/// `(72,46)` - see `vahn_house_roundtrip_disc.rs`.
const INTERIOR: (i16, i16) = (12480, 1344);
const EXIT_LANDING: (i16, i16) = (4672, 3008);

/// The kind-0 exit does not teleport on the crossing: it arms a `0x26`-frame
/// timer behind a fade to black, the pad is off while it runs and for `0x28`
/// frames after, and the player walks in place the whole time
/// (`FUN_801D1EC4`, `FUN_801D1344`).
#[test]
fn town01_kind0_exit_lands_after_the_warp_timer() {
    let Some(mut host) = open_town01() else {
        return;
    };
    let s = host.world.player_actor_slot.expect("player") as usize;
    host.world.actors[s].move_state.world_x = INTERIOR.0;
    host.world.actors[s].move_state.world_z = INTERIOR.1;
    drive(&mut host, 0, 2);
    // Walk toward the exit until the crossing arms the warp.
    let mut armed_at = None;
    for f in 0..120u32 {
        host.world.set_pad(PadButton::Down.mask());
        host.tick().expect("tick");
        if host.world.field_warp_in_flight() {
            armed_at = Some(f);
            break;
        }
    }
    assert!(
        armed_at.is_some(),
        "walking onto (97,9) arms the kind-0 warp"
    );
    let at_cross = {
        let ms = &host.world.actors[s].move_state;
        (ms.world_x, ms.world_z)
    };
    assert!(
        host.world.presentation.fade.is_some(),
        "the fade to black is up"
    );
    let walk_slot = slot(&host);
    // The warp runs: position held, pad ignored, the walk clip kept.
    let mut landed_after = None;
    for f in 1..=0x30u32 {
        host.world.set_pad(PadButton::Down.mask());
        host.tick().expect("tick");
        let ms = &host.world.actors[s].move_state;
        if (ms.world_x, ms.world_z) == EXIT_LANDING {
            landed_after = Some(f);
            break;
        }
        assert_eq!(
            (ms.world_x, ms.world_z),
            at_cross,
            "frame {f}: the pad is off mid-warp"
        );
        assert_eq!(slot(&host), walk_slot, "frame {f}: the clip is held");
    }
    assert_eq!(
        landed_after,
        Some(0x26),
        "the landing comes 0x26 frames after the crossing frame"
    );
    // The post-warp hold: the pad stays off for 0x28 more frames.
    drive(&mut host, PadButton::Down.mask(), 0x20);
    let ms = &host.world.actors[s].move_state;
    assert_eq!(
        (ms.world_x, ms.world_z),
        EXIT_LANDING,
        "the hold keeps the pad off after the landing"
    );
}
