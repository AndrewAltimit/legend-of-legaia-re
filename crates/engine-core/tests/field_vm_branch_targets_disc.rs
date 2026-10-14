//! Every PC the port's field VM computes on a shipped script lands on an
//! instruction boundary.
//!
//! The field VM (`FUN_801DE840`) has one family of conditional arms that
//! leave through a shared exit (`0x801E3608` / `0x801E360C` / `0x801E3614`)
//! adding an operand halfword less two to a PC already past the instruction:
//! a skip **relative** to the end of the instruction. Reading that halfword
//! as an absolute target, or measuring it from the wrong base, sends a
//! script into the middle of an instruction. A retail-correct reading has a property that is cheap to test without a
//! single capture: across the whole disc, every taken branch lands on an
//! instruction boundary of the record it sits in.
//!
//! The test walks every shipped script carrier (each scene's bundle MAN,
//! its streaming variant MANs and its event-script records), decodes each
//! record with the field disassembler, and runs **the port's own `step`**
//! on every instruction of the record's clean prefix under two forcing
//! hosts - one that answers every condition query "yes" and one that
//! answers "no". Each `Advance` the VM returns is then classified:
//!
//! * the next instruction or another decoded boundary - fine;
//! * past the record's clean prefix - unverifiable, counted;
//! * inside the clean prefix but not on a boundary - **a misread skip or a
//!   width the port and the disassembler disagree on**, and the test fails.
//!
//! The same rule catches a port arm whose width differs from the decoder's,
//! because its fall-through lands mid-instruction too.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use legaia_engine_field::man_field_scripts::partition_record_span;
use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_vm::field::{
    FieldActorFxHost, FieldActorHost, FieldActorSpawnHost, FieldAudioHost, FieldCameraHost,
    FieldCameraMotionHost, FieldCtx, FieldDialogMenuHost, FieldFlagsHost, FieldHaltHost,
    FieldPartyInventoryHost, FieldPlayerHost, FieldSceneHost, FieldScreenHost, FieldTerrainHost,
    Op49State, StepResult, step,
};
use legaia_scene_formats::field_disasm::{LinearWalker, OpKey, op_key};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// A host whose every condition query gives one answer. `yes` drives each
/// conditional arm down the side that takes its branch where the query
/// polarity allows; the numeric queries go to the extreme that makes a
/// `0x4E` compare true in mode 0 (`yes`) or mode 1 (`!yes`).
struct ForcedHost {
    yes: bool,
    global: u32,
}

impl ForcedHost {
    fn new(yes: bool) -> Self {
        Self {
            yes,
            global: if yes { u32::MAX } else { 0 },
        }
    }
    fn extreme(&self) -> i32 {
        if self.yes { i32::MIN } else { i32::MAX }
    }
}

impl FieldFlagsHost for ForcedHost {
    fn global_flags(&self) -> u32 {
        self.global
    }
    fn set_global_flags(&mut self, value: u32) {
        self.global = value;
    }
    fn frame_delta(&self) -> u16 {
        1
    }
    fn extra_flags(&self) -> u32 {
        if self.yes { u32::MAX } else { 0 }
    }
    fn held_pad(&self) -> u32 {
        if self.yes { u32::MAX } else { 0 }
    }
    fn pad_compass_entry(&self, _index: u8) -> Option<u32> {
        self.yes.then_some(0xF000)
    }
    fn slot_table_read(&self, _slot: u8) -> i16 {
        if self.yes { i16::MIN } else { i16::MAX }
    }
    fn system_flag_test(&self, _idx: u16) -> bool {
        self.yes
    }
    fn flag_bit_is_set(&self, _ctx: &FieldCtx, _bank: u8, _bit: u8) -> bool {
        self.yes
    }
}
impl FieldSceneHost for ForcedHost {}
impl FieldTerrainHost for ForcedHost {
    fn outside_world_bbox(&self, _ctx: &FieldCtx, _bbox: [i16; 4]) -> bool {
        self.yes
    }
}
impl FieldCameraHost for ForcedHost {}
impl FieldCameraMotionHost for ForcedHost {
    fn camera_glide_live(&mut self) -> bool {
        self.yes
    }
}
impl FieldScreenHost for ForcedHost {}
impl FieldAudioHost for ForcedHost {}
impl FieldPartyInventoryHost for ForcedHost {
    fn char_hp_mp_pair(&self, _page: u8, _sub_op: u8) -> (i32, i32) {
        (self.extreme(), 1)
    }
    fn party_bank_value(&self, _sub_op: u8) -> i32 {
        self.extreme()
    }
    fn char_level(&self, _page: u8) -> i32 {
        self.extreme()
    }
    fn op4c_nd_subc_item_held(&self, _needle: u8) -> bool {
        self.yes
    }
    fn op4c_nd_sube_item_worn(&self, _needle: u8) -> bool {
        self.yes
    }
    fn op4c_n8_subb_spell_known(&self, _spell: u8) -> bool {
        self.yes
    }
    fn op4c_n8_subd_skill_known(&self, _char_idx: u8, _skill: u8) -> bool {
        self.yes
    }
}
impl FieldDialogMenuHost for ForcedHost {
    fn op49_state(&self) -> Op49State {
        Op49State::Done
    }
}
impl FieldActorHost for ForcedHost {
    fn op4c_n8_subc_field_68_is_zero(&self, _ctx: &FieldCtx) -> bool {
        self.yes
    }
}
impl FieldActorSpawnHost for ForcedHost {
    fn actor_guard(&mut self, _actor_id: u8) -> Option<()> {
        (!self.yes).then_some(())
    }
}
impl FieldActorFxHost for ForcedHost {}
impl FieldPlayerHost for ForcedHost {}
impl FieldHaltHost for ForcedHost {}

/// Per-op tally of what the port's PCs did.
#[derive(Default)]
struct Tally {
    steps: usize,
    /// A PC other than the decoder's fall-through that lands on a boundary.
    branches_on_boundary: usize,
    /// A PC past the record's clean prefix (nothing to check it against).
    unverifiable: usize,
    /// A PC strictly inside the instruction that computed it: a zero or
    /// near-zero skip, which only filler records author (`26 00 00` jumps
    /// to `pc + 1` under any reading). Reported, not failed.
    inside_self: usize,
    /// A PC inside the clean prefix that is not a boundary.
    misaligned: usize,
    panics: usize,
    samples: Vec<String>,
}

/// `true` when `at` starts an op-`0x40` data block that ends exactly at
/// `end`. The decoder folds the block an op-`0x34` light carries into that
/// instruction; retail consumes it in the same step when it builds a new
/// light (`0x801DFFEC..0x801E0018`) and leaves it for the `0x40` op itself
/// when the light already exists (`0x801DFF48`), so both PCs are real.
fn embedded_data_block(body: &[u8], at: usize, end: usize) -> bool {
    body.get(at) == Some(&0x40)
        && body
            .get(at + 1)
            .is_some_and(|&len| at + 2 + usize::from(len) == end)
}

#[derive(Default)]
struct Audit {
    by_op: BTreeMap<OpKey, Tally>,
    /// Shipped forms of the compare-and-skip ops, by the operand that
    /// picks the condition: `0x4E`'s loader (`mode >> 4`) and compare
    /// (`mode & 0xF`), `0x42`'s mode and selector.
    forms: BTreeMap<String, usize>,
    records: usize,
}

impl Audit {
    fn record(&mut self, label: &str, body: &[u8], pc0: usize) {
        if pc0 >= body.len() {
            return;
        }
        self.records += 1;
        let mut clean = Vec::new();
        let mut clean_end = body.len();
        for item in LinearWalker::new(body, pc0) {
            match item {
                Ok(insn) => clean.push(insn),
                Err((pc, _)) => {
                    clean_end = pc;
                    break;
                }
            }
        }
        let boundaries: BTreeSet<usize> = clean
            .iter()
            .map(|i| i.pc)
            .chain(std::iter::once(clean_end))
            .collect();
        for insn in &clean {
            let key = op_key(insn);
            let operand = insn.pc + if insn.extended.is_some() { 2 } else { 1 };
            let form = match insn.opcode {
                0x4E => body
                    .get(operand + 1)
                    .map(|m| format!("4E loader {:X} compare {:X}", m >> 4, m & 0xF)),
                0x42 => body
                    .get(operand..operand + 2)
                    .map(|b| format!("42 mode {} selector {:02X}", b[0], b[1])),
                _ => None,
            };
            if let Some(form) = form {
                *self.forms.entry(form).or_default() += 1;
            }
            for yes in [true, false] {
                let mut host = ForcedHost::new(yes);
                let mut ctx = FieldCtx::default();
                let result = catch_unwind(AssertUnwindSafe(|| {
                    step(&mut host, &mut ctx, body, insn.pc)
                }));
                let t = self.by_op.entry(key).or_default();
                t.steps += 1;
                let next_pc = match result {
                    Ok(StepResult::Advance { next_pc }) => next_pc,
                    Ok(StepResult::Yield { resume_pc }) => resume_pc,
                    Ok(_) => continue,
                    Err(_) => {
                        t.panics += 1;
                        if t.samples.len() < 6 {
                            t.samples.push(format!("{label} +0x{:X} panicked", insn.pc));
                        }
                        continue;
                    }
                };
                if next_pc == insn.pc + insn.size {
                    continue;
                }
                let end = insn.pc + insn.size;
                if boundaries.contains(&next_pc) || embedded_data_block(body, next_pc, end) {
                    t.branches_on_boundary += 1;
                } else if next_pc > insn.pc && next_pc < end {
                    t.inside_self += 1;
                } else if next_pc > clean_end || next_pc < pc0 {
                    t.unverifiable += 1;
                } else {
                    t.misaligned += 1;
                    if t.samples.len() < 6 {
                        let end = (insn.pc + insn.size).min(body.len());
                        let bytes: Vec<String> = body[insn.pc..end]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect();
                        t.samples.push(format!(
                            "{label} +0x{:X} [{}] (yes={yes}) -> 0x{next_pc:X} (decoded size {})",
                            insn.pc,
                            bytes.join(" "),
                            insn.size
                        ));
                    }
                }
            }
        }
    }
}

#[test]
fn every_field_vm_branch_lands_on_an_instruction_boundary() {
    let Some(ex) = extracted_dir() else { return };
    let index = ProtIndex::open_extracted(&ex).expect("open ProtIndex");
    let mut audit = Audit::default();
    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            if !seen.insert(man.clone()) {
                continue;
            }
            let Ok(man_file) = legaia_scene_formats::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                for rec in 0..count {
                    if let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, rec)
                    {
                        let label = format!("{name} P{partition}[{rec}]");
                        audit.record(&label, &man[start..start + len], pc0);
                    }
                }
            }
        }
        if let Some(es) = scene.find_event_scripts() {
            for (i, &(a, b)) in es.record_ranges.iter().enumerate() {
                audit.record(&format!("{name} ev[{i}]"), &es.bytes[a..b], 0);
            }
        }
    }
    assert!(
        audit.records > 1000,
        "walked only {} records",
        audit.records
    );

    let mut failing = Vec::new();
    for (key, t) in &audit.by_op {
        if t.branches_on_boundary + t.unverifiable + t.inside_self + t.misaligned + t.panics == 0 {
            continue;
        }
        eprintln!(
            "[{key:>5}] steps {:>6}  branch-on-boundary {:>5}  unverifiable {:>4}  inside-self {:>3}  misaligned {:>4}  panics {:>3}",
            t.steps, t.branches_on_boundary, t.unverifiable, t.inside_self, t.misaligned, t.panics
        );
        for s in &t.samples {
            eprintln!("          {s}");
        }
        if t.misaligned + t.panics > 0 {
            failing.push(key.label());
        }
    }
    for (form, n) in &audit.forms {
        eprintln!("[form] {form}: {n}");
    }
    eprintln!("[ok] {} records walked", audit.records);
    assert!(
        failing.is_empty(),
        "the port computes a PC off an instruction boundary for: {failing:?}"
    );
}
