//! `FUN_801DE840`, the field VM, one instruction at a time over every script on
//! the disc, against `legaia_engine_vm::field::step`: with every call stubbed and
//! every flag clear, do the two agree where the next instruction is? A width or a
//! jump read wrong desynchronises a whole script, and nothing else in this crate
//! would see it.

use crate::common::{disc, scus_cpu};
use legaia_engine_vm::field::*;
use legaia_retail_exec::{GP, Stop};
use legaia_scene_formats::field_disasm::{LinearWalker, man_script_spans, op_key};
use std::collections::{BTreeMap, BTreeSet};

const VM: u32 = 0x801D_E840;
const OVERLAY_BASE: u32 = 0x801C_E818;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
const RESOLVE_ACTOR: u32 = 0x8003_C83C;
const READ_U16: u32 = 0x8003_CE9C;
const PACKET_LENGTH: u32 = 0x8003_CA38;

struct Inert;
impl FieldFlagsHost for Inert {
    fn global_flags(&self) -> u32 {
        0
    }
    fn set_global_flags(&mut self, _value: u32) {}
    fn frame_delta(&self) -> u16 {
        1
    }
}
impl FieldSceneHost for Inert {}
impl FieldTerrainHost for Inert {}
impl FieldCameraHost for Inert {}
impl FieldCameraMotionHost for Inert {}
impl FieldScreenHost for Inert {}
impl FieldAudioHost for Inert {}
impl FieldPartyInventoryHost for Inert {}
impl FieldDialogMenuHost for Inert {}
impl FieldActorHost for Inert {}
impl FieldActorSpawnHost for Inert {}
impl FieldActorFxHost for Inert {}
impl FieldPlayerHost for Inert {}
impl FieldHaltHost for Inert {}

fn man_payloads(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = legaia_scene_formats::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start < buf.len()
                && let Ok((decoded, _)) =
                    legaia_lzs::decompress_tracked(&buf[start..], d.size as usize)
            {
                out.push(decoded);
            }
        }
    }
    if let Ok(report) = legaia_container_formats::parse_streaming(buf, 4096) {
        for chunk in report.chunks.iter().filter(|c| c.type_byte == 0x03) {
            let start = chunk.header_offset + 4;
            if let Some(payload) = buf.get(start..start.saturating_add(chunk.size as usize)) {
                out.push(payload.to_vec());
            }
        }
    }
    out
}

/// Every distinct instruction on the disc, once, with the script around it.
fn corpus() -> Vec<(Vec<u8>, usize, String)> {
    let disc = disc().unwrap();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut out = Vec::new();
    // The scenes end before the battle data.
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
                continue;
            };
            for (_, _, start, pc0, len) in man_script_spans(&man_file, &man) {
                let body = &man[start..start + len];
                for step in LinearWalker::new(body, pc0) {
                    let Ok(insn) = step else { break };
                    let bytes = body[insn.pc..insn.pc + insn.size].to_vec();
                    if seen.insert(bytes.clone()) {
                        // Enough of the record for a jump to land in it.
                        let lo = insn.pc.saturating_sub(0x800);
                        let hi = (insn.pc + 0x800).min(body.len());
                        out.push((body[lo..hi].to_vec(), insn.pc - lo, op_key(&insn).label()));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_instruction_on_the_disc_steps_where_retail_steps() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX);
    // The operand reader is no side effect: it stays what it is.
    cpu.hook(READ_U16, |r, m| r.r[2] = u32::from(m.rd16(r.r[4])));
    cpu.hook(PACKET_LENGTH, |r, m| {
        let bytes = m.read(r.r[4], 0x400);
        r.r[2] = legaia_scene_formats::field_disasm::packet_length(&bytes) as u32;
    });
    cpu.step_budget = 20_000;
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let corpus = corpus();
    assert!(corpus.len() > 2000, "{}", corpus.len());

    let mut agree = 0u32;
    let mut stopped: BTreeMap<String, u32> = BTreeMap::new();
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in &corpus {
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, script);
        // A context whose script is running.
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        let stop = cpu.call(VM, &[SCRIPT, *pc as u32, CTX]);
        if stop != Stop::Returned {
            *stopped.entry(format!("{label} {stop:?}")).or_default() += 1;
            continue;
        }
        // The caller keeps the PC in a halfword: a backward jump's `addu` of the
        // unsigned offset wraps there.
        let retail = cpu.regs.r[2] as usize & 0xFFFF;
        let mut ctx = FieldCtx::default();
        let port = match step(&mut Inert, &mut ctx, script, *pc) {
            StepResult::Advance { next_pc } => Some(next_pc),
            StepResult::Yield { resume_pc } => Some(resume_pc),
            StepResult::Halt { final_pc } => Some(final_pc),
            StepResult::Pending { .. } | StepResult::Unknown { .. } => None,
        };
        if port.map(|p| p & 0xFFFF) == Some(retail) {
            agree += 1;
        } else {
            let waits = if retail == *pc { "retail waits " } else { "" };
            let e = differ.entry(format!("{waits}{label}")).or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!(
                    "pc {pc:#x} retail {retail:#x} port {port:x?} bytes {:02x?}",
                    &script[*pc..(*pc + 12).min(script.len())]
                );
            }
        }
    }
    assert!(agree as usize > corpus.len() * 99 / 100, "{agree}");
    // Only a walk over a stubbed tile list and the GTE stop a call.
    assert!(
        stopped
            .keys()
            .all(|k| k.starts_with("43 ") || k.starts_with("4C 83")),
        "{stopped:?}"
    );
    // What is left turns on state the two sides do not share: an inert host's
    // answers against stubbed calls and clear RAM, not on where the operands end.
    // `34 1x` spawned or not, `4C CD` / `D0` / `EA` / `53` / `35` / `4A` gates,
    // `4C DC` a list test, `4C EB` an actor lookup (the resolver hook finds one).
    const STATE_ARMS: [&str; 9] = [
        "34 10",
        "4C CD",
        "4C DC",
        "4C EA",
        "4C EB",
        "retail waits 35",
        "retail waits 4A",
        "retail waits 4C 53",
        "retail waits 4C D0",
    ];
    let report: Vec<String> = differ
        .iter()
        .filter(|(k, _)| !STATE_ARMS.contains(&k.as_str()))
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}
