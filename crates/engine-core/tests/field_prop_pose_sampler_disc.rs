//! Disc-gated: the posed-prop pose both hosts draw goes through the frame
//! blender's port (`field_env::prop_bone_offsets` over
//! `PropAnim::pose_key`, retail `FUN_8001BE80`).
//!
//! For every placed, posed prop of every CDNAME scene it sweeps the whole
//! cursor range of the prop's clip under both values of the clamp bit and
//! checks:
//!
//! - at every whole-frame cursor the kernel's pose equals the single-entry
//!   decode the hosts used to draw (`bone_transform(anim - 1, cursor >> 4)`),
//!   so no keyframe moves;
//! - a prop whose clip carries no blend gate poses that same decode at every
//!   sub-frame cursor too;
//! - the rest key is frame 0.
//!
//! It prints how many props carry the gate and how many swept samples the
//! blend changes. Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env::{self, ANIM_CLAMP, PropAnimBank, PropPoseKey};
use legaia_engine_core::scene::{ProtIndex, Scene};

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
fn every_posed_prop_keeps_its_keyframes_through_the_blender() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let (mut scenes, mut props, mut gated_props) = (0usize, 0usize, 0usize);
    let (mut samples, mut changed, mut keyframes) = (0usize, 0usize, 0usize);
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let (Ok(Some(placements)), Ok(Some(binds)), Ok(Some(man))) = (
            scene.field_object_placements(&index),
            scene.field_object_binds(&index),
            scene.field_man_payload(&index),
        ) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let Some(bundle) = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7]
                .into_iter()
                .find_map(|d| legaia_asset::player_anm::find_in_entry(&e.bytes, d).pop())
        }) else {
            continue;
        };
        let clip = |anim: u8| -> Option<(u16, bool, u8)> {
            let r = bundle.record((anim - 1) as usize).ok()?;
            Some((r.frame_count, r.blends(), (r.flag & 0xFF) as u8))
        };
        let bank = PropAnimBank::build(&placements, &binds, &man_file, &man, clip);
        if bank.props.is_empty() {
            continue;
        }
        scenes += 1;
        for (anchor, p) in &bank.props {
            let Ok(rec) = bundle.record(p.anim.anim_id as usize - 1) else {
                continue;
            };
            props += 1;
            gated_props += usize::from(rec.blends());
            assert_eq!(rec.blends(), p.anim.scaled_step, "{name} {anchor:?}");
            let bones = rec.bone_count as usize;
            let single = |frame: usize| {
                (0..bones)
                    .map(|b| {
                        let t = bundle
                            .bone_transform(p.anim.anim_id as usize - 1, frame, b)
                            .unwrap();
                        (
                            [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                            [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                        )
                    })
                    .collect::<Vec<_>>()
            };
            let rest =
                field_env::prop_bone_offsets(&bundle, p.anim.anim_id, PropPoseKey::REST, bones);
            assert_eq!(rest, Some(single(0)), "{name} {anchor:?} rest");
            for clamp in [false, true] {
                let mut a = p.anim;
                a.flags = if clamp {
                    a.flags | ANIM_CLAMP
                } else {
                    a.flags & !ANIM_CLAMP
                };
                for c in 0..(i32::from(rec.frame_count) * 16) {
                    a.cursor = c as i16;
                    let key = a.pose_key();
                    let got = field_env::prop_bone_offsets(&bundle, a.anim_id, key, bones).unwrap();
                    let old = single((c >> 4) as usize);
                    samples += 1;
                    if c & 0xF == 0 || !rec.blends() {
                        assert_eq!(got, old, "{name} {anchor:?} cursor {c} clamp {clamp}");
                        keyframes += usize::from(c & 0xF == 0);
                    } else if got != old {
                        changed += 1;
                    }
                }
            }
        }
    }
    eprintln!(
        "[ok] {scenes} scenes, {props} posed props ({gated_props} blend-gated); \
         {keyframes} whole-frame samples exact; the blend changes {changed} of {samples} swept samples"
    );
    assert!(props > 0, "no posed props found - the oracle is vacuous");
}
