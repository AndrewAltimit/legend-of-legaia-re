//! Disc-gated: EVERY move-FX effect-prototype record drives through the ported
//! move VM without hitting an unimplemented opcode.
//!
//! `move_fx_records_real` proves the 61-entry effect-prototype table
//! (`0x801F6324`, PROT 0898) decodes to summon-format part records, and
//! `move_fx_render_disc` drives *one* move (id `0x06`) end-to-end. This closes
//! the gap: it seeds every unique record exactly as the retail part-stager does
//! (`FUN_80021B04`: PC = 2 → bytecode at `record+4`) and runs it through the
//! same move VM the engine uses (`move_vm::step`, the `SUMMON_PART_BUDGET`
//! per-frame cap, the `wait_timer` gate), asserting none returns
//! `StepResult::Pending` — i.e. the engine can animate the whole authored
//! move-FX set, not just the worked example.
//!
//! It also reports the opcode coverage the corpus exercises (a real spread of
//! move-VM ops, not a trivial halt-immediately set). Skips and passes without
//! `LEGAIA_DISC_BIN` / `extracted/` (the disc-gated convention).

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::move_power::{self, BATTLE_ACTION_OVERLAY_PROT_INDEX};
use legaia_engine_vm::move_vm::{self, ActorState, MoveHost, StepResult};
use legaia_prot::archive::Archive;

/// The move VM's per-frame opcode cap (mirrors `engine_core::summon`).
const PER_FRAME_BUDGET: usize = 256;
/// Frames to drive each record (well past any real move-FX scene length).
const FRAMES: usize = 600;

/// A host with every callback at its no-op default — opcode dispatch still
/// runs, so an unimplemented opcode surfaces as `StepResult::Pending`.
struct NoopHost;
impl MoveHost for NoopHost {}

fn overlay_0898() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if !prot.is_file() {
            continue;
        }
        let mut archive = Archive::open(&prot).ok()?;
        let entry = archive
            .entries
            .get(BATTLE_ACTION_OVERLAY_PROT_INDEX)
            .cloned()?;
        let mut bytes = Vec::new();
        archive.read_entry(&entry, &mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

/// Drive one record (whole-buffer move program from `record_off`, PC = 2)
/// through the move VM. Returns `(reached_halt, opcodes_seen)` or the first
/// unimplemented `opcode` as `Err`.
fn drive_record(overlay: &[u8], record_off: usize) -> Result<(bool, BTreeSet<u16>), u16> {
    let buf: Vec<u16> = overlay[record_off..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut state = ActorState::new();
    state.pc = 2; // FUN_80021B04 seats the move buffer at PC 2 (record+4).
    state.wait_timer = -1; // run the VM on the first frame.
    let mut host = NoopHost;
    let mut seen = BTreeSet::new();

    for _frame in 0..FRAMES {
        move_vm::decrement_wait_timer(&mut state, 1);
        if state.wait_timer >= 0 {
            continue; // wait-timer gate (actor_tick's `bgez` skip).
        }
        for _ in 0..PER_FRAME_BUDGET {
            if let Some(&op) = buf.get(state.pc as usize).filter(|&&op| op <= 0x46) {
                seen.insert(op);
            }
            match move_vm::step(&mut host, &mut state, &buf) {
                StepResult::Advance => continue,
                StepResult::Halt | StepResult::EndOfBuffer { .. } => {
                    return Ok((true, seen));
                }
                StepResult::Wait => break, // deferred to next frame.
                StepResult::Pending { opcode } => return Err(opcode),
            }
        }
    }
    Ok((false, seen)) // never broke — fine, as long as no opcode was Pending.
}

#[test]
fn every_move_fx_record_runs_without_an_unimplemented_opcode() {
    let Some(overlay) = overlay_0898() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing");
        return;
    };

    let parts = move_power::parse_effect_proto_records(&overlay)
        .expect("effect-proto records decode from the real overlay");
    assert!(
        parts.len() >= 50,
        "expected the full move-FX record set (got {})",
        parts.len()
    );

    let mut all_ops = BTreeSet::new();
    let mut halted = 0usize;
    let mut failures: Vec<(usize, u16)> = Vec::new();

    for p in &parts {
        match drive_record(&overlay, p.record_off) {
            Ok((reached_halt, ops)) => {
                if reached_halt {
                    halted += 1;
                }
                all_ops.extend(ops);
            }
            Err(opcode) => failures.push((p.record_off, opcode)),
        }
    }

    assert!(
        failures.is_empty(),
        "move-FX records hit unimplemented move-VM opcodes: {}",
        failures
            .iter()
            .map(|(off, op)| format!("record {off:#x} → op {op:#04x}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // The corpus must exercise a real spread of opcodes (it isn't all
    // halt-immediately records) — sanity that the drive actually ran programs.
    assert!(
        all_ops.len() >= 6,
        "move-FX records exercised only {} distinct opcodes",
        all_ops.len()
    );

    eprintln!(
        "validated {} move-FX records through the move VM: {halted} reached halt/EOB, \
         {} distinct opcodes exercised {:#04x?}",
        parts.len(),
        all_ops.len(),
        all_ops,
    );
}
