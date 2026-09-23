//! Disc-gated census: which shipped move programs issue move-VM op `0x03`
//! (`WORLD_ROTATE_ADD`, `0x80023184`) - the one move-VM opcode that reads the
//! trig LUT pair (`_DAT_8007B81C` sine for X, `_DAT_8007B7F8` cosine for Z).
//!
//! Until `World::new` filled `World::sin_lut` / `cos_lut`, the live world's
//! `MoveHost::rotation_lut` answered `(0, 0)` and every op `0x03` was a no-op
//! step. This census names what that silenced: the per-scene prescript stager
//! records (ambient field-fx parts) and the slot-B summon stagers
//! (`0903..=0934`, one per player Seru-magic id), walked through the real
//! decoder so a hit is an instruction boundary, not a coincidental word.
//!
//! A measurement, not a pinned count: the test prints every carrier and
//! asserts only that the walk decoded instructions at all.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::scene_event_scripts::move_stager_records;
use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::summon::summon_stager_prot_entry;
use legaia_engine_vm::move_vm::{ActorState, MoveHost, StepResult, step};

const BUDGET: usize = 20_000;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

struct NullHost;
impl MoveHost for NullHost {}

fn words_of(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Walk one program from `pc`, following `Wait` breaks, and return
/// `(instructions decoded, op 0x03 count)`.
fn walk(words: &[u16], pc: i16) -> (usize, usize) {
    let mut host = NullHost;
    let mut state = ActorState::new();
    state.pc = pc;
    let mut seen = HashSet::new();
    let (mut n, mut hits) = (0usize, 0usize);
    for _ in 0..BUDGET {
        let pc = state.pc as usize;
        if pc >= words.len() || !seen.insert(pc) {
            break;
        }
        if words[pc] == 0x03 {
            hits += 1;
        }
        n += 1;
        match step(&mut host, &mut state, words) {
            StepResult::Advance | StepResult::Wait => {}
            _ => break,
        }
    }
    (n, hits)
}

#[test]
fn census_move_op_03_carriers() {
    let Some(root) = extracted_root() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&root.join("CDNAME.TXT")).expect("cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut decoded = 0usize;
    let (mut scenes, mut stager_records, mut stager_hits) = (0usize, 0usize, 0usize);
    let mut carriers = Vec::new();
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let scene = host.scene.as_ref().expect("scene loaded");
        scenes += 1;
        if let Some(scripts) = scene.find_event_scripts()
            && let Some(records) = move_stager_records(scripts.bytes)
        {
            for (id, rec) in records.iter().enumerate() {
                let words = words_of(&scripts.bytes[rec.record_off..rec.bytecode.end]);
                if words.len() < 3 {
                    continue;
                }
                stager_records += 1;
                let (n, hits) = walk(&words, 2);
                decoded += n;
                if hits > 0 {
                    stager_hits += 1;
                    carriers.push(format!("{name} prescript record {id}: {hits} x op 0x03"));
                }
            }
        }
    }

    let (mut summon_parts, mut summon_hits) = (0usize, 0usize);
    let prot = root.join("PROT.DAT");
    if let Ok(mut archive) = legaia_prot::archive::Archive::open(&prot) {
        for spell in 0x81u8..=0xA0 {
            let Some(idx) = summon_stager_prot_entry(spell) else {
                continue;
            };
            let entry = archive.entries[idx as usize].clone();
            let next = archive.entries[idx as usize + 1].clone();
            let mut bytes = Vec::new();
            if archive.read_entry(&entry, &mut bytes).is_err() {
                continue;
            }
            let len =
                summon_overlay::unique_content_len(bytes.len(), entry.start_lba, next.start_lba);
            bytes.truncate(len);
            let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
            for (pi, part) in overlay.parts.iter().enumerate() {
                let Some(b) = bytes.get(part.bytecode.clone()) else {
                    continue;
                };
                summon_parts += 1;
                let (n, hits) = walk(&words_of(b), 0);
                decoded += n;
                if hits > 0 {
                    summon_hits += 1;
                    carriers.push(format!(
                        "summon 0x{spell:02X} (PROT {idx:04}) part {pi}: {hits} x op 0x03"
                    ));
                }
            }
        }
    }

    for line in &carriers {
        eprintln!("  {line}");
    }
    eprintln!(
        "[ok] op 0x03 census: {stager_hits} of {stager_records} stager records over \
         {scenes} scenes, {summon_hits} of {summon_parts} summon parts"
    );
    assert!(
        decoded > 0,
        "the walk decoded nothing - census would be vacuous"
    );
}
