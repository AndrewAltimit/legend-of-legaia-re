//! Disc-gated RE harness + regression: pin the name-entry trigger in the
//! `town01` opening cutscene timeline.
//!
//! The opening Rim Elm sequence runs from a `town01` partition-2
//! (cutscene-timeline) record: establishing camera, Vahn's scripted walk-out,
//! then the "Select your name." prompt. This installs that record as a spawned
//! [`legaia_engine_core::cutscene_timeline::CutsceneTimeline`] and decodes it
//! through the authoritative field VM ([`legaia_engine_vm::field::step`]). The
//! VM is the only complete decoder: the `field_disasm` linear decoder bails on
//! unported `0x4C` sub-ops, and a naive linear disassembler mis-strides the
//! variable-width `0x4C` menu-control op.
//!
//! ## Finding (pinned)
//!
//! The opening is **partition-2 record 3** (`P2[3]`). After the establishing
//! camera sweep and Vahn's walk-out, it suspends on **op `0x49` STATE_RESUME
//! sub-op 3 at body offset `0x02c6` (`49 03 00`)** - the field-VM instruction
//! that hands off to the name-entry overlay. Confirmed against the
//! `name_input_ui` save-state oracle (fingerprint `a14afa51…`): while name
//! entry is open, `_DAT_8007B450` (the op-0x49 state slot) holds `0x800EB297`,
//! which is `(address of that 0x49 op) + 1` - the record is loaded with body
//! `0x02b0` at RAM `0x800EB280` and the bytes byte-match exactly. So the field
//! script is parked precisely at this `0x49` while name entry is up. See
//! `docs/subsystems/cutscene.md` and `docs/reference/open-rev-eng-threads.md`.
//!
//! Run with `--nocapture` to read the full op stream.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use legaia_engine_vm::field::{FieldCtx, FieldHost, Op49State, StepResult, step};
use std::path::PathBuf;

/// Minimal field host for a *decode-only* linear walk of a record body.
///
/// It forces the two host-gated stalls to advance so the walk reaches every
/// instruction: a huge [`FieldHost::frame_delta`] satisfies WAIT_FRAMES (0x4A)
/// in one step, and [`FieldHost::op49_state`] returns `Done` so STATE_RESUME
/// (0x49) advances by its encoded width instead of arming. The remaining
/// unconditional structural park (`0x4C` sub-D `script_alloc`) is stepped past
/// by [`linear_decode`]. No game state is simulated - this only enumerates
/// instruction boundaries (and the VM's op decode is authoritative).
#[derive(Default)]
struct LinearTraceHost {
    flags: u32,
}

impl FieldHost for LinearTraceHost {
    fn global_flags(&self) -> u32 {
        self.flags
    }
    fn set_global_flags(&mut self, value: u32) {
        self.flags = value;
    }
    fn frame_delta(&self) -> u16 {
        0x7FFF
    }
    fn op49_state(&self) -> Op49State {
        Op49State::Done
    }
}

/// One decoded instruction: `(pc, opcode, extended-target, size)`.
#[derive(Debug, Clone, Copy)]
struct DecodedOp {
    pc: usize,
    opcode: u8,
    extended: Option<u8>,
    size: usize,
}

/// Linear-decode `bytecode` from `pc0` through the authoritative field VM,
/// stepping past blocking/structural halts so the whole forward run is decoded.
/// Stops at the first backward jump (a loop) or the end of the buffer.
fn linear_decode(bytecode: &[u8], pc0: usize) -> Vec<DecodedOp> {
    let mut host = LinearTraceHost::default();
    let mut ctx = FieldCtx {
        script_id: 0xFB,
        ..FieldCtx::default()
    };
    let mut out = Vec::new();
    let mut pc = pc0;
    let mut guard = 0;
    while pc < bytecode.len() && guard < 8000 {
        guard += 1;
        let op_byte = bytecode[pc];
        let extended = (op_byte & 0x80 != 0).then(|| bytecode.get(pc + 1).copied().unwrap_or(0));
        let hs = if op_byte & 0x80 != 0 { 2 } else { 1 };
        let next = match step(&mut host, &mut ctx, bytecode, pc) {
            StepResult::Advance { next_pc } => next_pc,
            StepResult::Yield { resume_pc } => resume_pc,
            StepResult::Halt { final_pc } if final_pc != pc => final_pc,
            // Parked structural halt (e.g. `0x4C` sub-D script-alloc) or an
            // op this port can't advance past: step by its encoded width
            // (these read only `op0`).
            StepResult::Halt { .. } | StepResult::Pending { .. } | StepResult::Unknown { .. } => {
                pc + hs + 1
            }
        };
        out.push(DecodedOp {
            pc,
            opcode: op_byte & 0x7F,
            extended,
            size: next.saturating_sub(pc),
        });
        if next <= pc {
            break; // backward jump → end of forward pass
        }
        pc = next;
    }
    out
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Body offset of the name-entry STATE_RESUME (`49 03 00`) in `town01` P2[3].
const NAME_ENTRY_OP49_OFFSET: usize = 0x02c6;

#[test]
fn town01_opening_name_entry_trigger_is_op49_at_0x02c6() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");

    let man_bytes = host
        .scene
        .as_ref()
        .expect("town01 scene")
        .field_man_payload(&host.index)
        .expect("man payload result")
        .expect("town01 has a field MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse town01 MAN");

    // Install the opening record (partition-2 record 3) and grab its body.
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man_bytes, 2, 3, true),
        "town01 P2[3] (the opening record) installs as a cutscene timeline"
    );
    let tl = host
        .world
        .cutscene_timeline
        .take()
        .expect("timeline present");
    let pc0 =
        legaia_engine_core::man_field_scripts::partition_record_span(&man_file, &man_bytes, 2, 3)
            .expect("P2[3] span")
            .1;

    let ops = linear_decode(&tl.bytecode, pc0);

    // Dump the forward op stream (skip the trailing NOP/JMP idle loop padding).
    eprintln!("[town01 P2[3]] forward op stream ({} insns):", ops.len());
    for op in &ops {
        if matches!(op.opcode, 0x21 | 0x26) {
            continue;
        }
        let ext = op
            .extended
            .map(|t| format!("*{t:02X}"))
            .unwrap_or_else(|| "  ".into());
        let mark = match op.opcode {
            0x49 => "  <== STATE_RESUME (name-entry handoff)",
            _ => "",
        };
        eprintln!(
            "  {:#06x} op={:02X}{ext} sz={}{mark}",
            op.pc, op.opcode, op.size
        );
    }

    // The pin: the authoritative forward decode reaches body offset 0x02c6 as
    // a real STATE_RESUME (op 0x49) instruction - the 3-byte sub-op-3 form
    // (`49 03 00`) - proving it is an instruction boundary, not operand data
    // inside a `0x4C` payload. This is the site the `name_input_ui` save's
    // op-0x49 armed-PC (`_DAT_8007B450` = 0x800EB297 = this op's RAM address+1)
    // points at while name entry is open. (The record carries two further
    // STATE_RESUMEs deeper in the opening for later beats; only this one is the
    // name-entry handoff, fixed by the save correlation.)
    let trigger = ops
        .iter()
        .find(|o| o.pc == NAME_ENTRY_OP49_OFFSET)
        .copied()
        .expect("the forward decode reaches body offset 0x02c6 as an instruction boundary");
    assert_eq!(
        trigger.opcode, 0x49,
        "the name-entry trigger op is STATE_RESUME (0x49)"
    );
    assert!(
        trigger.extended.is_none(),
        "the trigger 0x49 is a same-context op (not the 0x80 cross-context form)"
    );
    assert_eq!(
        trigger.size, 3,
        "it is the 3-byte sub-op-3 form (`49 03 00`)"
    );
    assert_eq!(
        tl.bytecode
            .get(NAME_ENTRY_OP49_OFFSET..NAME_ENTRY_OP49_OFFSET + 3),
        Some(&[0x49u8, 0x03, 0x00][..]),
        "the bytes at the pinned offset are `49 03 00`"
    );
}
