//! Disc-gated census: where a scene's entry script aims a cross-context op at a
//! placed object whose bind prologue leaves it **halted** on a compass walk.
//!
//! The field dispatcher refuses an extended op on a halted target unless the
//! caller is the system script (`+0x50 == 0xFB`, `FUN_801DE840`
//! `0x801DE938..0x801DE940`). The port tested the target's id, so these ops were
//! skipped; `conc2`'s light shaft (`P0[11]`, `37 02 01` in its prologue, parked
//! by `P1[0]`'s `A3 0B 7F 7F`) is the case that drew wrong. This census lists
//! every (scene, record, op) of that shape on the disc - the ops the fix lets
//! through - and pins the set.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{LinearWalker, partition_record_span};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Whether the record's spawn prologue (its ops up to the first `0x21`) runs a
/// compass walk on its own actor (`0x37` / `0x41` without the `0x80` prefix),
/// which halts it until the walk lands.
fn prologue_walks(body: &[u8], pc0: usize) -> bool {
    for insn in LinearWalker::new(body, pc0).map_while(Result::ok) {
        let op = body[insn.pc];
        if op == 0x21 {
            return false;
        }
        if op == 0x37 || op == 0x41 {
            return true;
        }
    }
    false
}

/// The system-script ops the port routes to an object's own context
/// (`World::step_field_cross_context_actor_op`): `A3` seat, `AB` / `AC`
/// local-flag writes, `CC <id> 35|36|51`.
fn routed(body: &[u8], pc: usize) -> bool {
    match body[pc] {
        0xA3 | 0xAB | 0xAC => true,
        0xCC => matches!(body.get(pc + 2), Some(0x35 | 0x36 | 0x51)),
        _ => false,
    }
}

#[test]
fn entry_script_ops_on_walking_objects() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open prot index");
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("CDNAME");
    let names: Vec<String> = cdname
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some("#define")).then(|| w.next().map(str::to_string))?
        })
        .collect();

    let mut hits = BTreeSet::new();
    let mut scenes = 0usize;
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        scenes += 1;
        let walking: BTreeSet<u8> = (0..mf.partitions[0].len())
            .filter(|&r| {
                partition_record_span(&mf, &man, 0, r)
                    .is_some_and(|(start, pc0, len)| prologue_walks(&man[start..start + len], pc0))
            })
            .filter_map(|r| u8::try_from(r).ok())
            .collect();
        if walking.is_empty() {
            continue;
        }
        let Some((start, pc0, len)) = partition_record_span(&mf, &man, 1, 0) else {
            continue;
        };
        let body = &man[start..start + len];
        for insn in LinearWalker::new(body, pc0).map_while(Result::ok) {
            let pc = insn.pc;
            let Some(&target) = body.get(pc + 1) else {
                continue;
            };
            if routed(body, pc) && walking.contains(&target) {
                hits.insert(format!("{name} P0[{target}] {:02X}@{pc:#x}", body[pc]));
            }
        }
    }
    eprintln!("[halted-targets] {scenes} scenes with a MAN; entry-script ops on walking objects:");
    for h in &hits {
        eprintln!("[halted-targets]   {h}");
    }
    assert!(scenes >= 100, "scenes with a MAN: {scenes}");
    assert!(
        hits.contains("conc2 P0[11] A3@0x87"),
        "conc2's light-shaft park is one: {hits:?}"
    );
    assert_eq!(hits.len(), EXPECTED_HITS, "{hits:?}");
}

/// Entry-script ops on a walking object, disc-wide.
const EXPECTED_HITS: usize = 4;
