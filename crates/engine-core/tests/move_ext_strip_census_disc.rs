//! Disc-gated census: which shipped move programs issue move-VM extension
//! sub-op `0x2C` - the call into the scanline strip emitter `FUN_801D31B0`
//! (PROT 0897; ported as `legaia_engine_vm::move_ext_strip`).
//!
//! Sub-op `0x2F`/`0x2C` executes only while the field overlay is resident
//! (the dispatcher `FUN_801D362C` exists in 0897 alone), so the carriers a
//! field scene can run are the two per-scene move-record tables:
//!
//! * the prescript stager table (`move_stager_records`, installed by field-VM
//!   op `FUN_800252EC`), walked here through the real move-VM decoder so a
//!   hit is an instruction boundary, never a coincidental word pair;
//! * the scene bundle's MOVE payload (`_DAT_8007B888`), scanned for the
//!   aligned `[0x002F, 0x002C]` pair.
//!
//! The census is a measurement, not an assertion of a count: the test prints
//! every carrier and asserts only that the walk itself decoded extension
//! instructions (so a zero is a real zero, not a broken walker).
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::scene_event_scripts::move_stager_records;
use legaia_engine_core::scene::SceneHost;
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

#[derive(Default)]
struct StripRecorder {
    strip_calls: usize,
    ext_ops: usize,
    sub_ops: std::collections::BTreeMap<u16, usize>,
}

impl MoveHost for StripRecorder {
    fn ext_func801d31b0(&mut self, _state: &mut ActorState, _operand: &[u16]) {
        self.strip_calls += 1;
    }
}

fn words_of(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Walk one stager record (PC = 2, past `[model_sel][reserved]`), following
/// the decoder across `Wait` breaks, until it halts, leaves the record or
/// revisits a PC.
fn walk(words: &[u16]) -> StripRecorder {
    let mut host = StripRecorder::default();
    let mut state = ActorState::new();
    state.pc = 2;
    let mut seen = HashSet::new();
    for _ in 0..BUDGET {
        let pc = state.pc as usize;
        if pc >= words.len() || !seen.insert(pc) {
            break;
        }
        if words[pc] == 0x2F {
            host.ext_ops += 1;
            if let Some(&sub) = words.get(pc + 1) {
                *host.sub_ops.entry(sub).or_default() += 1;
            }
        }
        match step(&mut host, &mut state, words) {
            StepResult::Advance | StepResult::Wait => {}
            _ => break,
        }
    }
    host
}

fn aligned_pair_hits(words: &[u16]) -> usize {
    words.windows(2).filter(|w| w == &[0x2F, 0x2C]).count()
}

#[test]
fn census_move_ext_sub_op_2c_carriers() {
    let Some(root) = extracted_root() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&root.join("CDNAME.TXT")).expect("cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut scenes = 0usize;
    let mut records_walked = 0usize;
    let mut ext_ops = 0usize;
    let mut sub_ops = std::collections::BTreeMap::<u16, usize>::new();
    let mut payload_sub_ops = std::collections::BTreeMap::<u16, usize>::new();
    let mut strip_records = Vec::new();
    let mut move_payload_hits = Vec::new();
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
                records_walked += 1;
                let r = walk(&words);
                ext_ops += r.ext_ops;
                for (k, v) in &r.sub_ops {
                    *sub_ops.entry(*k).or_default() += v;
                }
                if r.strip_calls > 0 || aligned_pair_hits(&words) > 0 {
                    strip_records.push(format!(
                        "{name} prescript record {id}: decoded 0x2C calls {} / aligned pairs {}",
                        r.strip_calls,
                        aligned_pair_hits(&words)
                    ));
                }
            }
        }
        if let Some(bundle) = legaia_engine_core::scene_bundle::find_bundle(scene)
            && let Ok(extended) = host.index.entry_bytes_extended(bundle.entry_idx())
            && let Ok(Some(payload)) =
                legaia_engine_core::scene_bundle::extract_move_payload(&bundle, &extended)
        {
            let hits = aligned_pair_hits(&words_of(&payload));
            if hits > 0 {
                move_payload_hits.push(format!("{name} MOVE payload: aligned pairs {hits}"));
            }
        }
    }
    // Every PROT entry's type-0x05 (MOVE) slot, not just the CDNAME scenes'
    // bundles - the kingdom bundles' slot 4 is the world map's move root.
    let mut move_slots = 0usize;
    for idx in 0..host.index.entry_count() as u32 {
        let Ok(bytes) = host.index.entry_bytes_extended(idx) else {
            continue;
        };
        if let Some(Ok(payload)) =
            legaia_asset::scene_asset_table::decode_slot_by_type(&bytes, 0x05)
        {
            move_slots += 1;
            for w in words_of(&payload).windows(2) {
                if w[0] == 0x2F && w[1] < 0x3D {
                    *payload_sub_ops.entry(w[1]).or_default() += 1;
                }
            }
            let hits = aligned_pair_hits(&words_of(&payload));
            if hits > 0 {
                move_payload_hits.push(format!(
                    "PROT {idx:04} type-0x05 slot: aligned pairs {hits}"
                ));
            }
        }
    }
    eprintln!("  {move_slots} PROT entries carry a decodable type-0x05 slot");
    eprintln!("  decoded stager sub-op histogram: {sub_ops:x?}");
    eprintln!("  MOVE-slot aligned [0x2F, sub] pairs: {payload_sub_ops:x?}");
    for line in strip_records.iter().chain(&move_payload_hits) {
        eprintln!("  {line}");
    }
    eprintln!(
        "[ok] census over {scenes} scenes: {records_walked} stager records walked, \
         {ext_ops} decoded 0x2F ops, {} stager carriers of 0x2C, {} MOVE-payload carriers",
        strip_records.len(),
        move_payload_hits.len()
    );
    assert!(
        ext_ops > 0,
        "the walk decoded no 0x2F at all - census would be vacuous"
    );
}
