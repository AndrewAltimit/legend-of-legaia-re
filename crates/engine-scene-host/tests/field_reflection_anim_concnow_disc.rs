//! Disc-gated: the Room of Mirrors image in `concnow` walks and runs with the
//! player instead of gliding.
//!
//! `concnow` `P1[1]` installs a `4C 86` reflection controller naming the
//! player (`0xF8`) in its spawn prologue, after it re-stages its own mesh to
//! the party model `0xF0` (`4C 50 F0`), raises its party-bank bit (`31 18`)
//! and binds clip `2` (`22 02`). The controller's tick `FUN_801E5154` copies
//! the source's clip state onto the image whenever both actors' model words
//! `+0x64` agree (`0x801E5204..0x801E5250`): the party-bank bit
//! `+0x10 & 0x01000000`, the clip id `+0x5C`, the rate `+0x6A` and the cursor
//! `+0x68`. The port mirrored the position and facing only, so the image
//! slid along the far side of the glass on its idle clip.
//!
//! The test walks and runs the player inside the controller's rect, ticking
//! the scene the way a play host does (the clip cues drained against the
//! party locomotion bank each tick), and asserts that the image's clip is
//! the bank record the player plays and that its cursor follows the
//! player's. Structural assertions only - no Sony bytes.
//!
//! REF: FUN_801E5154 (`0x801E5204..0x801E5250`, the clip copy)
//!
//! Skip-passes without `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_field::actor_handler::ActorHandler;
use legaia_engine_field::field_anim::{FieldClipPlayer, FieldPlayerAnim};
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

/// `concnow` `P1[1]` - the player's image.
const IMAGE_SLOT: u8 = 1;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `(bank record + 1, cursor)` of the clip the player draws.
fn player_clip(host: &SceneHost) -> (u16, u32) {
    let anim = host.world.locomotion.player_anim.as_ref().expect("anim");
    let (source, id, cursor, _) = anim.playing_clip();
    let base = anim.leader * 7;
    let slot = match source {
        "bank" => id as u16,
        "walk" => 0,
        "idle" => 1,
        other => panic!("player plays a {other} clip in the mirror room"),
    };
    (base + slot + 1, cursor)
}

#[test]
fn concnow_mirror_image_plays_the_players_clip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open SceneHost");
    let loco = legaia_actor_formats::character_pack::field_locomotion_anm(
        &host
            .index
            .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
            .expect("PROT 0874"),
    )
    .expect("party locomotion bundle");
    host.enter_field_scene("concnow", 0).expect("enter concnow");
    host.world.mode = legaia_engine_mode::mode::SceneMode::Field;
    let scene_anm =
        legaia_engine_scene::npc_catalog::scene_anm_bundle(host.scene.as_ref().unwrap());
    let anim = FieldPlayerAnim::from_locomotion_bank(&loco, 0).expect("Vahn's bank");
    host.world.set_field_player_anim(Some(anim));

    // One host frame: the session tick, then the clip-cue drain a play host
    // runs against its banks (`drain_field_anim_cues`), then the shared NPC
    // clip step over the world's cursors.
    let party_slots: Vec<u8> = (0..8).collect();
    let frame = |host: &mut SceneHost, pad: u16| {
        host.world.set_pad(pad);
        host.tick().expect("tick");
        let _ = host
            .world
            .drain_field_anim_cues(scene_anm.as_ref(), Some(&loco), |s| {
                Some(party_slots.contains(&s))
            });
    };

    // Stand through the load hold; the image's prologue seats its mirror.
    for _ in 0..0x60 {
        frame(&mut host, 0);
    }
    let seated = host
        .world
        .actors
        .iter()
        .filter(|a| a.active && a.handler == ActorHandler::Reflection)
        .count();
    assert_eq!(seated, 3, "concnow seats its three mirrors at entry");
    // The clip copy's gate is the two ends' model words: each image
    // re-stages itself (`4C 50 F0` / `F1` / `F2`) to the model its source
    // spawns with - the player's lead form, and the party placements
    // `P1[2]` / `P1[3]` the scene's story beats walk to the glass.
    for (image, source, model) in [(4u8, 2usize, 0xF1i16), (5, 3, 0xF2)] {
        assert_eq!(host.world.field_npc_live_model(image), Some(model));
        let spawn = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == source)
            .map(|c| c.ctx.model_id as i16);
        assert_eq!(
            spawn,
            Some(model),
            "P1[{source}] spawns as model {model:#x}"
        );
    }
    assert_eq!(host.world.field_npc_live_model(IMAGE_SLOT), Some(0xF0));
    // What a host does at scene entry: bind the image's spawn clip.
    if let Some(a) = host.world.field_npc_live_anim(usize::from(IMAGE_SLOT))
        && let Some(p) = FieldClipPlayer::from_record(&loco, usize::from(a.max(1)) - 1)
    {
        host.world.bind_npc_clip_cursor(IMAGE_SLOT, a, &p);
    }

    // Seat the player inside the controller's rect (tiles 30..=38 x
    // 90..=110), clear of the walk-on row 103.
    let s = host.world.player_actor_slot.expect("player") as usize;
    host.world.actors[s].move_state.world_x = 35 * 128 + 64;
    host.world.actors[s].move_state.world_z = 96 * 128 + 64;
    host.world.actors[s].physics.world_x = 35 * 128 + 64;
    host.world.actors[s].physics.world_z = 96 * 128 + 64;
    host.world.actors[s].move_state.flags &= !0x0008_0000;

    let mut checked = 0usize;
    let mut kinds = std::collections::BTreeSet::new();
    for (pad, frames) in [
        (0u16, 20u32),
        (PadButton::Left.mask(), 24),
        (0, 12),
        (PadButton::Right.mask() | PadButton::R1.mask(), 16),
        (PadButton::Up.mask(), 16),
    ] {
        for i in 0..frames {
            frame(&mut host, pad);
            let (px, pz) = host.world.player_field_position().expect("player");
            let (tx, tz) = ((px + 0x40) >> 7, (pz + 0x40) >> 7);
            assert!(
                (30..=38).contains(&tx) && (90..=110).contains(&tz),
                "player left the mirror rect at ({tx}, {tz})"
            );
            if i < 6 {
                // A clip switch rebinds the image on the tail of the
                // frame; give it the game frame after (the player's clip
                // steps on the frame grid) to pick the cursor up.
                continue;
            }
            let (clip, cursor) = player_clip(&host);
            let image = host
                .world
                .npcs
                .clip_cursors
                .get(&IMAGE_SLOT)
                .copied()
                .expect("the image's clip is bound");
            eprintln!(
                "pad {pad:#06x} f{i}: player clip {clip} cursor {cursor} / image clip {} cursor {}",
                image.anim_id, image.cursor
            );
            assert_eq!(
                u16::from(image.anim_id),
                clip,
                "the image plays the player's clip"
            );
            // Within one game frame's step: the player's clip steps on the
            // frame grid (`FUN_800204F8` once a frame, times the frame step),
            // and the image copies the cursor the frame before it.
            let player_step = host
                .world
                .locomotion
                .player_anim
                .as_ref()
                .and_then(|a| a.playing_step())
                .unwrap_or(1);
            let step = player_step.max(image.rate.max(1) as u32)
                * u32::from(host.world.clock.frame_step.max(1));
            let d = (image.cursor.max(0) as u32).abs_diff(cursor);
            let span = u32::from(image.frames) * 16;
            assert!(
                d <= step || span.saturating_sub(d) <= step,
                "the image's cursor {} follows the player's {cursor} (step {step})",
                image.cursor
            );
            kinds.insert(clip);
            checked += 1;
        }
    }
    assert!(checked > 40, "non-vacuous");
    assert!(
        kinds.len() >= 3,
        "idle, walk and run all reached the image: {kinds:?}"
    );
}
