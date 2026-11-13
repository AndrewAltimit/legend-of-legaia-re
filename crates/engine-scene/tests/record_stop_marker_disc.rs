//! Disc-gated: an executed `0x21` ends a partition-2 record.
//!
//! A spawned record runs in an actor context whose tick is `FUN_8003BC08`
//! (the template `FUN_8003BDE0` allocates at `0x8003C068`), and that tick
//! steps the script through `FUN_80039B7C` only while the context's engaged
//! bit `+0x10 & 0x100` is up. The runner leaves its loop on a raw `0x21`
//! (`0x80039E20`) and clears the bit (`0x80039E68..0x80039E7C`), so the
//! record stops there for good: the spawn links the context into the list
//! at `0x8007C34C`, and the id resolver `FUN_8003C83C` walks `0x8007C354`,
//! so no `B1 <id> 08` reaches it to re-engage it. The bytes after such a
//! stop run only when a jump lands on them.
//!
//! `rikuroa` `P2[54]` (Terra's crawl and Noa's "will you take me with you?")
//! is the case that shows it: its Yes arm plays out and stops on a `0x21`,
//! and the bytes after it are the No arm's brooding and second ask, which the
//! first picker's No target reaches by a jump. The test pins that shape and
//! lists every partition-2 record on the disc with a stop that code follows,
//! without baking any page text into the source. Skip-passes without disc
//! data / extracted assets (CLAUDE.md convention).

use legaia_engine_field::man_field_scripts::partition_record_span;
use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{InsnInfo, decode};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// A record's stop markers: each executed `0x21`, with whether any code
/// follows it that is not the record's closing idle loop (more `21`s and a
/// backward `26`).
/// A `0x21` closing a picker's choice list is the list's terminator, not an
/// op the record executes: the picker's arms jump past it.
fn stops(body: &[u8], pc0: usize) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    let mut pc = pc0;
    let mut in_picker = false;
    while pc < body.len() {
        let Ok(insn) = decode(body, pc) else {
            pc += 1;
            continue;
        };
        if matches!(insn.info, InsnInfo::Picker { .. }) {
            in_picker = true;
        }
        if insn.size == 1 && body[pc] == 0x21 {
            if in_picker {
                in_picker = false;
            } else {
                let mut next = pc + 1;
                while body.get(next) == Some(&0x21) {
                    next += 1;
                }
                let tail = next >= body.len()
                    || decode(body, next).is_ok_and(
                        |n| matches!(n.info, InsnInfo::JmpRel { target, .. } if target <= next),
                    );
                out.push((pc, !tail));
            }
        }
        pc += insn.size.max(1);
    }
    out
}

#[test]
fn rikuroa_noa_join_yes_arm_stops_before_the_no_arm() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload fetch")
        .expect("rikuroa has a MAN payload");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("man parse");
    let (start, pc0, len) = partition_record_span(&mf, &man, 2, 54).expect("P2[54] span");
    let body = &man[start..start + len];

    // The first ask: a two-way picker whose Yes arm jumps forward to the
    // join, and whose No arm runs on into the brooding.
    let mut pc = pc0;
    let mut first = None;
    while pc < body.len() {
        let insn = decode(body, pc).expect("P2[54] decodes");
        if let InsnInfo::Picker {
            count: 2, targets, ..
        } = insn.info
        {
            first = Some((targets[0], targets[1]));
            break;
        }
        pc += insn.size;
    }
    let (yes, no) = first.expect("P2[54] asks");
    assert!(yes < no, "the Yes arm comes first");
    // The Yes arm ends on a forward jump to the join.
    let mut pc = yes;
    let join = loop {
        let insn = decode(body, pc).expect("the Yes arm decodes");
        if let InsnInfo::JmpRel { target, .. } = insn.info {
            break target;
        }
        pc += insn.size;
    };
    assert!(join > no, "Yes jumps past the No arm to the join");

    // The join plays out and stops on a `0x21` with code after it: the
    // No arm's second ask, which only the first picker's jumps reach.
    let s = stops(body, pc0);
    let stop = s
        .iter()
        .find(|&&(p, followed)| p > join && followed)
        .map(|&(p, _)| p)
        .expect("the join stops on a 0x21 with code after it");
    let mut pc = stop + 1;
    let mut second_ask = false;
    while pc < body.len() {
        let insn = decode(body, pc).expect("past the stop decodes");
        if matches!(insn.info, InsnInfo::Picker { count: 2, .. }) {
            second_ask = true;
            break;
        }
        pc += insn.size;
    }
    assert!(second_ask, "the code past the stop asks again");

    // Every partition-2 record on the disc with a stop that code follows.
    let mut followed = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
                continue;
            };
            let n2 = mf.header.partition_counts[2].max(0) as usize;
            for r in 0..n2 {
                let Some((s, pc0, l)) = partition_record_span(&mf, man, 2, r) else {
                    continue;
                };
                let Some(body) = man.get(s..s + l) else {
                    continue;
                };
                let n = stops(body, pc0).iter().filter(|x| x.1).count();
                if n > 0 {
                    followed.push(format!("{name} P2[{r}] x{n}"));
                }
            }
        }
    }
    followed.sort();
    followed.dedup();
    eprintln!(
        "{} partition-2 records stop with code after the stop: {followed:?}",
        followed.len()
    );
    assert!(followed.iter().any(|s| s.starts_with("rikuroa P2[54]")));
}
