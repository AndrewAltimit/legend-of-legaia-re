//! Disc-gated: a model re-stage re-sizes the actor's skeleton, so the clip a
//! script binds right after it lands.
//!
//! `dolk` `P2[11]` is the pens beat after the Genesis Tree revives: the
//! people held as monsters turn back. It re-models placements `0x20..=0x25`
//! (`CC <id> 50 <model> 00`), drops their party-bank bit (`B2 <id> 18`) and
//! binds a scene-bank clip (`A2 <id> <clip>`). Two of them go from a
//! ten- and a seven-part monster to a four-part person (`CC 24 50 85 00` then
//! `A2 24 1B`, `CC 25 50 86 00` then `A2 25 1C`). Retail's re-stage
//! (`FUN_80024E08` through `FUN_80020F88`) rebuilds the render node from the
//! new TMD (`FUN_80024D78` writes its object count), so the four-bone clip
//! poses the four-part mesh. The port kept the old bone count and refused the
//! clip, drawing the new mesh under the monster's pose: the seated servant's
//! head sunk into his robe (#592).
//!
//! Skip-passes without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn a_remodelled_placement_takes_the_new_skeletons_clip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "dolk".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    s.enter_field_live("dolk", &FieldLiveOpts::default())
        .expect("enter dolk");
    let b = legaia_engine_session::scene_actors::bind_scene_actors(&mut s.host);
    assert!(s.play_scene_record(2, 11), "dolk P2[11] installs");

    // (slot, clip, bones) of every re-target the cue drain hands a host.
    let mut bound = Vec::new();
    for _ in 0..1200u32 {
        s.host.world.set_pad(0);
        // The scene tick alone: the session's own tail drains the cues
        // with no banks, as a headless driver draws nothing.
        let _ = s.host.tick();
        let out = s.host.world.drain_field_anim_cues(
            b.scene_anm.as_ref(),
            b.locomotion_anm.as_ref(),
            |slot| b.clip_targets.get(&slot).copied(),
        );
        for r in out {
            let model = s.host.world.field_npc_live_model(r.slot);
            bound.push((r.slot, model, r.player.bone_count()));
        }
    }
    // The four-part people the beat re-models to (models 133 and 134).
    for model in [133i16, 134] {
        let hit = bound.iter().find(|&&(_, m, _)| m == Some(model)).copied();
        assert!(
            hit.is_some_and(|(_, _, bones)| bones == 4),
            "model {model}'s four-bone clip binds after the re-model: {bound:?}"
        );
    }
}
