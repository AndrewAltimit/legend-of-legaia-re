//! Disc-gated: a placement's header clip steps at the rate its own spawn
//! section wrote.
//!
//! The clip advance reads the actor's `+0x6A` on every tick (`lh
//! v0,0x6a(s0)` at `0x800205C8` on the gated arm, `lh a0,0x6a(s0)` at
//! `0x800205EC` on the plain one, in `FUN_800204F8`). `garmel` `P1[5]`
//! (flat 29, Zeto) opens its spawn section with `4C 41 08` and `P1[7]`
//! (flat 31) with `4C 41 04`: rates `4` and `2`, half and a quarter of the
//! template's `8`. `garmel_zeto_appears_rising` holds exactly those words on
//! the two actors, on header clips `55` and `56`.
//!
//! A host binds a header clip from its own clip player, at the step the
//! record gives the template rate; the tick then puts the cursor on the
//! actor's live word.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_field::field_anim::FieldClipPlayer;
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

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
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Placement, header clip and the rate its spawn section leaves on `+0x6A`.
const RATED: [(u8, u8, i16); 2] = [(5, 55, 4), (7, 56, 2)];

#[test]
fn garmel_header_clips_step_at_their_spawn_rate() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("garmel", 0).expect("enter garmel");
    let scene_anm =
        legaia_engine_scene::npc_catalog::scene_anm_bundle(host.scene.as_ref().expect("scene"))
            .expect("garmel's scene clip bank");
    for (slot, clip, rate) in RATED {
        let anim = host
            .world
            .field_npc_live_anim(usize::from(slot))
            .expect("header clip");
        assert_eq!(anim, clip, "P1[{slot}]'s header clip");
        let ch = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == usize::from(slot))
            .expect("placement channel");
        assert_eq!(ch.ctx.field_6a, rate, "P1[{slot}]: `4C 41` ran at spawn");
        let player =
            FieldClipPlayer::from_record(&scene_anm, usize::from(anim) - 1).expect("clip player");
        host.world.bind_npc_clip_cursor(slot, anim, &player);
        let at = host.world.npcs.clip_cursors[&slot].cursor;
        host.world.tick_npc_clips();
        assert_eq!(
            host.world.npcs.clip_cursors[&slot].cursor - at,
            rate,
            "P1[{slot}] steps its header clip at the actor's own rate"
        );
    }
}
