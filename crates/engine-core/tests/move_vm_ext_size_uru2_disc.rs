//! Disc-gated regression: the move-VM's `0x2F` extension dispatcher must
//! advance the PC by each sub-op's real width, so no `0x2F` instruction ever
//! re-enters the outer dispatcher on its own sub-opcode word.
//!
//! The witness is `uru2`. Its prescript stager bundle carries a `2F 25 ....`
//! instruction - extension sub-op `0x25` (slot save), width 3 halfwords per
//! the `li s2, 0x3` exit slot at `0x801D4244`. Advancing by 1 instead lands
//! the PC on the `0x25` word itself, which the **outer** opcode space reads
//! as `CHILD_SPAWN` with the following word as its slot operand - a spawn of
//! stager record 0, which is the per-scene SFX descriptor bank and never a
//! move record. Because the record loops, that mis-decode re-fires every
//! tick, so the symptom is a runaway spawn rather than a one-off.
//!
//! The assertion is stated on the decoder, not on the ambient host: every PC
//! the outer dispatcher decodes an opcode at must be an instruction boundary,
//! never the sub-opcode word of a preceding `0x2F`.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::collections::HashSet;
use std::path::PathBuf;

use legaia_asset::scene_event_scripts::move_stager_records;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::move_vm::{ActorState, MoveHost, StepResult, step};

/// Opcode budget per record - large enough for a looping record to revisit
/// its whole body many times, small enough to bound a runaway.
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
struct SpawnRecorder {
    spawns: Vec<i16>,
}

impl MoveHost for SpawnRecorder {
    fn spawn_child(&mut self, _state: &mut ActorState, slot: i16) {
        self.spawns.push(slot);
    }
}

/// Walk one record's move bytecode and report the PCs at which the outer
/// dispatcher decoded an opcode, plus every `spawn_child` slot it reached.
fn walk_record(words: &[u16]) -> (Vec<usize>, Vec<i16>) {
    let mut host = SpawnRecorder::default();
    let mut state = ActorState::new();
    // `FUN_80021B04` stages PC = 2: the record's first two words are the
    // `[model_sel][flags]` header, not bytecode.
    state.pc = 2;
    let mut decoded = Vec::new();
    for _ in 0..BUDGET {
        let pc = state.pc as usize;
        if pc >= words.len() {
            break;
        }
        decoded.push(pc);
        match step(&mut host, &mut state, words) {
            StepResult::Advance => {}
            _ => break,
        }
    }
    (decoded, host.spawns)
}

#[test]
fn uru2_stager_records_never_re_enter_on_an_ext_sub_opcode_word() {
    let Some(root) = extracted_root() else {
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open PROT index");
    let scene = Scene::load(&index, "uru2").expect("load uru2");
    let scripts = scene
        .find_event_scripts()
        .expect("uru2 carries an event-script prescript");
    let records = move_stager_records(scripts.bytes).expect("uru2 stager records");

    // The carrier the runaway was measured on. Guard the index so a bundle
    // re-key surfaces as a clear failure rather than a silent skip.
    assert!(
        records.len() > 12,
        "uru2 stager bundle has {} records; expected the record-12 carrier",
        records.len()
    );

    let mut checked_ext_ops = 0usize;
    for (id, rec) in records.iter().enumerate() {
        let bytes = &scripts.bytes[rec.record_off..rec.bytecode.end];
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if words.len() < 3 {
            continue;
        }

        let (decoded, spawns) = walk_record(&words);

        // Every word one past a decoded `0x2F` is a sub-opcode, never an
        // instruction boundary.
        let sub_op_words: HashSet<usize> = decoded
            .iter()
            .filter(|&&pc| words.get(pc).copied() == Some(0x2F))
            .map(|&pc| pc + 1)
            .collect();
        checked_ext_ops += sub_op_words.len();
        for pc in &decoded {
            assert!(
                !sub_op_words.contains(pc),
                "uru2 record {id}: outer opcode decoded at word {pc}, which is the \
                 sub-opcode word of a preceding 0x2F - the extension dispatcher \
                 under-advanced the PC"
            );
        }

        // Record 0 is the per-scene SFX descriptor bank, not a move record.
        // The install op cannot name it (`FUN_800252EC` is called with
        // `arg + 1`), so a spawn of slot 0 can only come from a mis-decode.
        assert!(
            !spawns.contains(&0),
            "uru2 record {id}: spawned stager record 0 (the SFX descriptor \
             bank) - only a mis-decoded 0x2F sub-opcode word reaches it"
        );
    }

    assert!(
        checked_ext_ops > 0,
        "no 0x2F extension instruction decoded across uru2's stager records - \
         the regression would be vacuous"
    );
    eprintln!("[ok] uru2: {checked_ext_ops} decoded 0x2F instructions, no re-entry");
}
