//! Disc-gated census: the face-at bind of every `CC F8 85|8E|8F`
//! halt-acquire on the disc.
//!
//! A talk's acquire hands the player the walk kernel's FaceTarget leg
//! (`FUN_8003774C`, `0x80037DE0..0x80037EA8`): sub-mode at op `+2`, a `u16`
//! frame budget, and at op `+5` the bind of the actor to face - resolved like
//! any cross-context id, to the actor-list node whose `+0x50` equals it. The
//! placement spawner writes `+0x50 = N0 + placement_index`, so for a
//! partition-1 record the record's *own* actor is bind `N0 + index`.
//!
//! The one captured acquire (`retock`'s innkeeper, `CC F8 85 14 00 33`) faces
//! its own actor, and the port once resolved every face-at bind that way. This
//! census is why it no longer does: many placement records name another
//! actor, and object / cutscene records have no own actor at all. The runner
//! resolves the bind through the scene's channel set
//! (`World::talk_face_target`).
//!
//! Every carrier is walked (bundle + streaming variant MANs), every record of
//! every partition from its own first-opcode offset; only clean decodes count
//! (`legaia_asset::field_disasm::clean_hit_offsets`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_asset::field_disasm::{OpKey, clean_hit_offsets, man_script_spans};
use legaia_engine_core::man_field_scripts::scene_man_carriers;
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
fn face_at_binds_are_not_always_the_records_own_actor() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ex) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    // partition -> (sites, sites whose bind is the record's own actor)
    let mut tally: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut retock_innkeeper_own = false;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            let Ok(mf) = legaia_asset::man_section::parse(&c.payload) else {
                continue;
            };
            let n0 = mf.header.partition_counts[0].max(0) as usize;
            for (p, r, start, pc0, len) in man_script_spans(&mf, &c.payload) {
                let body = &c.payload[start..start + len];
                for sub in [0x85u8, 0x8E, 0x8F] {
                    let key = OpKey {
                        opcode: 0x4C,
                        sub: Some(sub),
                    };
                    for pc in clean_hit_offsets(body, pc0, key) {
                        let Some(op) = body.get(pc..pc + 6) else {
                            continue;
                        };
                        // The cross-context form on the player only.
                        if op[0] != 0xCC || op[1] != 0xF8 {
                            continue;
                        }
                        let own = p == 1 && usize::from(op[5]) == n0 + r;
                        let e = tally.entry(p).or_default();
                        e.0 += 1;
                        e.1 += usize::from(own);
                        if name == "retock" && own && op[5] == 0x33 {
                            retock_innkeeper_own = true;
                        }
                    }
                }
            }
        }
    }
    eprintln!("[ok] partition -> (acquires, own-actor binds): {tally:?}");
    let (p1_sites, p1_own) = tally.get(&1).copied().unwrap_or_default();
    assert!(
        retock_innkeeper_own,
        "the captured retock acquire faces its own actor"
    );
    assert!(p1_own > 0, "some placement acquires face their own actor");
    assert!(
        p1_own < p1_sites,
        "some placement acquires face another actor: {p1_own} of {p1_sites} are own"
    );
    let no_own: usize = tally
        .iter()
        .filter(|(p, _)| **p != 1)
        .map(|(_, (n, _))| n)
        .sum();
    assert!(
        no_own > 0,
        "object / cutscene records issue the acquire with no own actor"
    );
}
