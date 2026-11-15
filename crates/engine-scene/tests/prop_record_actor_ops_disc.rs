//! Disc-gated census: the partition-0 (placed-object) records whose examine
//! body writes the object's own model or morph, or turns the player.
//!
//! A `.MAP` placed object's record runs twice over: its bind-time prologue
//! on the object's bind channel (up to the first raw `21`), and its body on
//! touch or examine through the prop runner. Four op shapes in a body write
//! an actor the prop runner has to name:
//!
//! - own `4C 50 <lo> <hi>`: the executing actor's model (`FUN_80024E08`);
//! - own `4B <count> <base> ..`: the executing actor's VDF morph lanes;
//! - `CC F8 85|8E|8F <lo> <hi> <bind>`: the player's FaceTarget leg;
//! - `B8 F8 <dir> <frames>`: the player's RotateToAngle leg.
//!
//! The prop runner stepped all four on a stand-in context, so none of them
//! landed: `vell`'s Bridge Grass (`P0[1]`) stayed a sprout and Vahn never
//! turned. This walk lists every body site (clean decodes only,
//! `legaia_scene_formats::field_disasm::clean_hit_offsets`) and pins the
//! Bridge Grass among them.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{OpKey, clean_hit_offsets, man_script_spans};

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
fn prop_bodies_that_write_their_object_or_turn_the_player() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ex) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    let key = |opcode, sub| OpKey { opcode, sub };
    // (scene, record, shape, body offset)
    let mut sites: BTreeSet<(String, usize, &'static str, usize)> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            let Ok(mf) = legaia_scene_formats::man_section::parse(&c.payload) else {
                continue;
            };
            for (p, r, start, pc0, len) in man_script_spans(&mf, &c.payload) {
                if p != 0 {
                    continue;
                }
                let body = &c.payload[start..start + len];
                // The prologue ends at the first raw `21`.
                let body_from = clean_hit_offsets(body, pc0, key(0x21, None))
                    .into_iter()
                    .min()
                    .unwrap_or(usize::MAX);
                let mut add =
                    |shape: &'static str, pcs: Vec<usize>, want: &dyn Fn(&[u8]) -> bool| {
                        for pc in pcs {
                            if pc > body_from && want(&body[pc..]) {
                                sites.insert((name.clone(), r, shape, pc));
                            }
                        }
                    };
                add(
                    "4C 50",
                    clean_hit_offsets(body, pc0, key(0x4C, Some(0x50))),
                    &|b| b[0] == 0x4C,
                );
                add("4B", clean_hit_offsets(body, pc0, key(0x4B, None)), &|b| {
                    b[0] == 0x4B
                });
                for sub in [0x85u8, 0x8E, 0x8F] {
                    add(
                        "CC F8 85",
                        clean_hit_offsets(body, pc0, key(0x4C, Some(sub))),
                        &|b| b.len() >= 2 && b[0] == 0xCC && b[1] == 0xF8,
                    );
                }
                add(
                    "B8 F8",
                    clean_hit_offsets(body, pc0, key(0x38, None)),
                    &|b| b.len() >= 2 && b[0] == 0xB8 && b[1] == 0xF8,
                );
            }
        }
    }
    for (scene, r, shape, pc) in &sites {
        eprintln!("{scene:10} P0[{r:2}] {shape:9} +{pc:#06x}");
    }
    let records: BTreeSet<(&str, usize)> =
        sites.iter().map(|(s, r, _, _)| (s.as_str(), *r)).collect();
    eprintln!(
        "[ok] {} body sites in {} records",
        sites.len(),
        records.len()
    );
    for shape in ["4C 50", "4B", "CC F8 85", "B8 F8"] {
        assert!(
            sites
                .iter()
                .any(|(s, r, sh, _)| s == "vell" && *r == 1 && *sh == shape),
            "vell P0[1] carries a body {shape}"
        );
    }
}
