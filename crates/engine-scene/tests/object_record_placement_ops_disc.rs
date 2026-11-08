//! Disc-gated census: the object (partition-0) records that drive a
//! **placement's** actor words with a cross-context op.
//!
//! A door, counter or switch record often animates an NPC beside it - the
//! `koin3` dance-hall door (`P0[3]`) steps its doorman (`P1[2]`, id `0x26`)
//! into the doorway with `AC 26 07`, `AB 26 03`, `B2 26 18`, `A2 26 35`,
//! `AD 26 08`. The prop runner (`World::step_prop_interaction`) sends these
//! ops to the placement's own context; this census lists every carrier of the
//! shape so the routing has a denominator. Word ops: an `A2` ExecMove, an `A3`
//! seat, an `AB` / `AC` / `AD` clip-control op, a `B1` / `B2` class bit other
//! than retire (`3`), engage (`8`) and halt (`0xA`), or a `CC ..` sub-op on
//! the resolved actor (scale, height, look, tint, player copy, seat ...:
//! `stone`'s Shadow Gate records, `chitei2`'s generator panels). Motion
//! ops: a compass walk, turn, walk-to-tile or face-at, handed to the
//! placement's walk kernel. Both aimed at an id in `N0..N0 + N1`.
//!
//! Every scene's bundle MAN is walked, each partition-0 record from its own
//! first-opcode offset, clean decodes only.
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{DisasmError, LinearWalker, man_script_spans};

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// The `0x4C` sub-ops whose arm works on the resolved actor (the prologue's
/// `s5`), mirrored from `World`'s `is_resolved_actor_4c`.
fn resolved_actor_4c(sub: u8) -> bool {
    matches!(
        sub,
        0x35..=0x37 | 0x40..=0x45 | 0x48 | 0x50 | 0x51 | 0x81 | 0x8C | 0xC0 | 0xD9 | 0xDA | 0xE3 | 0xE4
    )
}

/// Whether the decoded op is one the prop runner routes onto a placement's
/// words.
fn routed(opcode: u8, body: &[u8], pc: usize) -> bool {
    match opcode {
        0x22 | 0x23 | 0x2B..=0x2D => true,
        0x4C => body.get(pc + 2).is_some_and(|&sub| resolved_actor_4c(sub)),
        0x31 | 0x32 => body
            .get(pc + 2)
            .is_some_and(|bit| !matches!(bit & 0x1F, 3 | 8 | 0xA)),
        _ => false,
    }
}

/// Whether the decoded op is a motion leg the prop runner hands the
/// placement's walk kernel: a compass walk (`37` / `41`), a turn (`38`), a
/// walk-to-tile (`47`) or a face-at (`4C 85` / `8E` / `8F`).
fn motion(opcode: u8, body: &[u8], pc: usize) -> bool {
    match opcode {
        0x37 | 0x38 | 0x41 | 0x47 => true,
        0x4C => matches!(body.get(pc + 2), Some(0x85 | 0x8E | 0x8F)),
        _ => false,
    }
}

#[test]
fn object_records_that_drive_a_placements_clip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(ex) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    // (scene, P0 record, P1 placement) -> (word ops, motion ops)
    let mut sites: BTreeMap<(String, usize, usize), (usize, usize)> = BTreeMap::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Some(c) = scene_man_carriers(&index, &scene).into_iter().next() else {
            continue;
        };
        let Ok(mf) = legaia_scene_formats::man_section::parse(&c.payload) else {
            continue;
        };
        let n0 = mf.header.partition_counts[0].max(0) as usize;
        let n1 = mf.header.partition_counts[1].max(0) as usize;
        for (p, r, start, pc0, len) in man_script_spans(&mf, &c.payload) {
            if p != 0 {
                continue;
            }
            let body = &c.payload[start..start + len];
            for step in LinearWalker::new(body, pc0) {
                match step {
                    Ok(insn) => {
                        let Some(t) = insn.extended.map(usize::from) else {
                            continue;
                        };
                        if !(n0..n0 + n1).contains(&t) {
                            continue;
                        }
                        let (word, legs) = (
                            routed(insn.opcode, body, insn.pc),
                            motion(insn.opcode, body, insn.pc),
                        );
                        if word || legs {
                            let e = sites.entry((name.clone(), r, t - n0)).or_default();
                            e.0 += usize::from(word);
                            e.1 += usize::from(legs);
                        }
                    }
                    Err((_, e)) => {
                        if !matches!(e, DisasmError::EndOfStream { .. }) {
                            break;
                        }
                    }
                }
            }
        }
    }
    for ((scene, r, slot), (word, legs)) in &sites {
        eprintln!("[census] {scene} P0[{r}] -> P1[{slot}]: {word} word ops, {legs} motion ops");
    }
    assert!(
        sites
            .get(&("koin3".to_string(), 3, 2))
            .is_some_and(|&(word, legs)| word > 0 && legs > 0),
        "koin3's dance-hall door drives and turns its doorman"
    );
    assert!(
        sites
            .iter()
            .any(|((s, _, _), &(_, legs))| s == "jiji" && legs > 0),
        "jiji's counters walk their keeper"
    );
    for scene in ["stone", "chitei2", "nilboa"] {
        assert!(
            sites.keys().any(|(s, _, _)| s == scene),
            "{scene}'s object records drive a placement's actor"
        );
    }
    let scenes: std::collections::BTreeSet<&str> =
        sites.keys().map(|(s, _, _)| s.as_str()).collect();
    assert!(
        scenes.len() > 1,
        "the shape is not koin3's alone: {scenes:?}"
    );
}
