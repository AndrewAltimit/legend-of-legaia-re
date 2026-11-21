//! `FUN_801DE840`, the field VM, one instruction at a time over every script on
//! the disc, against `legaia_engine_vm::field::step`: with every call stubbed and
//! every flag clear, do the two agree where the next instruction is? A width or a
//! jump read wrong desynchronises a whole script, and nothing else in this crate
//! would see it.

use crate::common::{disc, scus_cpu};
use legaia_engine_vm::field::*;
use legaia_scene_formats::field_disasm::{LinearWalker, man_script_spans, op_key};
use psx_retail_exec::{GP, Stop};
use std::collections::{BTreeMap, BTreeSet};

const VM: u32 = 0x801D_E840;
const OVERLAY_BASE: u32 = 0x801C_E818;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
const RESOLVE_ACTOR: u32 = 0x8003_C83C;
const READ_U16: u32 = 0x8003_CE9C;
const PACKET_LENGTH: u32 = 0x8003_CA38;

/// Answers what stubbed calls and clear RAM answer in the VM: every call returns
/// zero, the resolver finds a context, the frame step is `1`.
pub(crate) struct Inert {
    pub(crate) hit: bool,
}
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
impl FieldCameraMotionHost for Inert {
    fn camera_glide_live(&mut self) -> bool {
        false
    }
}
impl FieldScreenHost for Inert {}
impl FieldAudioHost for Inert {
    fn bgm_ready(&mut self, sub_op: u8) -> bool {
        // Sub-op `0xA` waits on a load-settled bit that clear RAM leaves down.
        sub_op != 0xA
    }
}
impl FieldPartyInventoryHost for Inert {
    fn op4c_nd_subc_item_held(&self, _needle: u8) -> bool {
        // The bag lookup's stub says slot `0`, or no slot.
        self.hit
    }
}
impl FieldDialogMenuHost for Inert {
    fn press_prompt(&mut self, _x: u16, _y: u16) -> bool {
        false
    }
    fn op4c_n5_sub3_dialog_wait(&mut self, _ctx: &mut FieldCtx) -> bool {
        // `FUN_801D65D8(1)` runs for real, off its first stage.
        true
    }
}
impl FieldActorHost for Inert {}
impl FieldActorSpawnHost for Inert {
    fn actor_guard(&mut self, _actor_id: u8) -> Option<()> {
        self.hit.then_some(())
    }
}
impl FieldActorFxHost for Inert {
    fn spawn_attached_light(
        &mut self,
        _ctx: &FieldCtx,
        _ext: Option<u8>,
        _spawn: &legaia_engine_vm::field_actor_billboard::AttachedSpriteSpawn,
        _script: Option<&[u8]>,
    ) -> bool {
        true
    }
}
impl FieldPlayerHost for Inert {}
impl FieldHaltHost for Inert {}

fn man_payloads(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = legaia_scene_formats::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start < buf.len()
                && let Ok((decoded, _)) =
                    psx_lzs::decompress_tracked(&buf[start..], d.size as usize)
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
pub(crate) fn corpus() -> Vec<(Vec<u8>, usize, String)> {
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
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX);
    // The operand reader is no side effect: it stays what it is, a signed
    // halfword (`sll 16; sra 16` at `0x8003CEAC`).
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.hook(PACKET_LENGTH, |r, m| {
        let bytes = m.read(r.r[4], 0x400);
        r.r[2] = legaia_scene_formats::field_disasm::packet_length(&bytes) as u32;
    });
    cpu.step_budget = 20_000;
    cpu.mem.wr8(0x1F80_0393, 1);
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
        let port = match step(&mut Inert { hit: true }, &mut ctx, script, *pc) {
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
            if e.0 == 1 {
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
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}

#[test]
fn the_staged_mode_switch_steps_as_retail_steps() {
    differential!(prot(897, 0x801D_65D8));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    // The sound and CD queue is idle.
    cpu.stub(0x8003_DE7C, 0);
    let mut port = dispatch_leaves::FieldModeSwitch::default();
    for (i, target) in [
        1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(cpu.call(0x801D_65D8, &[target]), Stop::Returned);
        let stepping = port.step(target);
        assert_eq!(cpu.regs.r[2] != 0, stepping, "call {i}");
        assert_eq!(cpu.mem.rd32(0x8007_BA3C), port.target, "call {i}");
        assert_eq!(cpu.mem.rd32(0x8007_B9C0), port.stage, "call {i}");
    }
}

#[test]
fn a_missing_actor_or_item_takes_the_skip_retail_takes() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = 0);
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.stub(0x8004_2EE0, 0x100);
    cpu.mem.wr8(0x1F80_0393, 1);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut ran = 0;
    for (script, pc, label) in corpus() {
        if label != "4C DC" && label != "4C EB" {
            continue;
        }
        ran += 1;
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        assert_eq!(cpu.call(VM, &[SCRIPT, pc as u32, CTX]), Stop::Returned);
        let retail = cpu.regs.r[2] as usize & 0xFFFF;
        let mut ctx = FieldCtx::default();
        let StepResult::Advance { next_pc } =
            step(&mut Inert { hit: false }, &mut ctx, &script, pc)
        else {
            panic!("{label} at {pc:#x} does not advance");
        };
        assert_eq!(next_pc & 0xFFFF, retail, "{label} at {pc:#x}");
    }
    assert!(ran > 50, "{ran}");
}

#[test]
fn an_inn_stay_restores_what_retail_restores() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX);
    let mut rng = crate::common::Rng(0x0049_4E4E);
    for round in 0..300u32 {
        let slot = round % 3;
        let rec = 0x8008_4708 + slot * 0x414;
        let bytes: Vec<u8> = (0..0x414).map(|_| rng.next() as u8).collect();
        cpu.mem.load(rec, &bytes);
        cpu.mem.load(SCRIPT, &[0x4C, 0x82, slot as u8]);
        let stop = cpu.call(VM, &[SCRIPT, 0, CTX]);
        assert_eq!(stop, Stop::Returned);
        assert_eq!(cpu.regs.r[2], 3);
        let after = cpu.mem.read(rec, 0x414);

        let mut member = legaia_save::Party::zeroed(1).members[0].clone();
        member.raw.copy_from_slice(&bytes);
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = hms.hp_max;
        hms.mp_cur = hms.mp_max;
        member.set_hp_mp_sp(hms);
        let diff = crate::common::diff_offsets(&after, &member.raw);
        assert!(diff.is_empty(), "round {round}: {diff:?}");
    }
}

/// `3C` / `3D`, the party join and leave, stepped by the real `World` on a
/// generated present-party list: retail's arms (`0x801E04C4..0x801E05DC`,
/// `0x801E05E0..0x801E06D0`) edit the list `0x80084598` for `0x80084594`
/// entries and the leader byte `0x80084597`; the port's are
/// `FieldHostImpl::party_add` / `party_remove` over `present_party_list`.
/// Compared: the list (count and order) and the leader. Lists run from empty
/// to full, sorted as the join keeps them, with the operand in or out of it.
#[test]
fn a_party_join_or_leave_edits_the_list_as_retail_does() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const COUNT: u32 = 0x8008_4594;
    const LEADER: u32 = 0x8008_4597;
    const LIST: u32 = 0x8008_4598;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x5041_5254);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for round in 0..2000u32 {
        let op = if round % 2 == 0 { 0x3C } else { 0x3D };
        let n = rng.below(5) as usize;
        let mut list: Vec<u8> = Vec::new();
        while list.len() < n {
            let id = rng.below(5) as u8;
            if !list.contains(&id) {
                list.push(id);
            }
        }
        list.sort_unstable();
        let id = if !list.is_empty() && rng.below(2) == 0 {
            list[rng.below(list.len() as u32) as usize]
        } else {
            rng.below(5) as u8
        };
        let leader = if !list.is_empty() && rng.below(3) != 0 {
            list[rng.below(list.len() as u32) as usize]
        } else {
            rng.below(5) as u8
        };

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.wr8(COUNT, list.len() as u8);
        cpu.mem.wr8(LEADER, leader);
        for i in 0..4 {
            cpu.mem
                .wr8(LIST + i, list.get(i as usize).copied().unwrap_or(0));
        }
        cpu.mem.load(SCRIPT, &[op, id]);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(cpu.regs.r[2], 2);
        let count = cpu.mem.rd8(COUNT) as usize;
        let retail_list = cpu.mem.read(LIST, count.min(4));
        let retail_leader = cpu.mem.rd8(LEADER);

        let mut w = legaia_engine_core::world::World::new();
        w.party.party_actor_slots = list.iter().map(|&i| Some(i)).collect();
        w.party.field_list_emptied = list.is_empty();
        w.party.party_leader_slot = Some(leader);
        w.field_bytecode = vec![op, id, 0x21];
        w.field_pc = 0;
        let _ = w.step_field();
        let port_list = w.present_party_list();
        let port_leader = w.party.party_leader_slot;
        if port_list != retail_list || port_leader != Some(retail_leader) {
            *differ
                .entry(format!(
                    "{op:02X} {id} on {list:?} leader {leader}: retail {retail_list:?} \
                     leader {retail_leader}, port {port_list:?} leader {port_leader:?}"
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// The party-record tests of op `4C`, stepped by the real `World` on
/// generated records and a generated present-party list: `4C 8B` (a spell in
/// the learned list `+0x13C` of a present member below character 3,
/// `0x801E2320..0x801E23E4`), `4C 8D` (a skill in character `n`'s displayed
/// list `+0x185`, `0x801E2404..0x801E2474`), `4C DE` (an item in a present
/// member's equipment `+0x196`, `0x801E2F7C..0x801E3024`) and `4C DC`, the
/// same scan behind a bag miss (`FUN_80042EE0` answers `0x100`; the port's
/// bag is empty). Compared: the next PC, the skip taken or not.
#[test]
fn a_spell_skill_or_item_test_skips_as_retail_skips() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const RECORDS: u32 = 0x8008_4708;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.stub(0x8004_2EE0, 0x100);
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x534B_494C);
    let (mut taken, mut ran) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for round in 0..4000u32 {
        let sub = [0x8B, 0x8D, 0xDE, 0xDC][round as usize % 4];
        let n = rng.below(5) as usize;
        let mut list: Vec<u8> = Vec::new();
        while list.len() < n {
            let id = rng.below(4) as u8;
            if !list.contains(&id) {
                list.push(id);
            }
        }
        list.sort_unstable();
        // Small id ranges, so a hit is common.
        let mut records = vec![0u8; 4 * 0x414];
        for r in records.chunks_mut(0x414) {
            for b in r.iter_mut() {
                *b = rng.next() as u8;
            }
            r[0x13C] = rng.below(12) as u8;
            r[0x185] = rng.below(12) as u8;
            for i in 0..12 {
                r[0x13D + i] = rng.below(24) as u8;
                r[0x186 + i] = rng.below(24) as u8;
            }
            for i in 0..8 {
                r[0x196 + i] = rng.below(24) as u8;
            }
        }
        let needle = rng.below(24) as u8;
        let ch = rng.below(4) as u8;
        let script: Vec<u8> = match sub {
            0x8D => vec![0x4C, sub, ch, needle, 0x10, 0x00, 0x21],
            _ => vec![0x4C, sub, needle, 0x10, 0x00, 0x21],
        };

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(RECORDS, &records);
        cpu.mem.wr8(0x8008_4594, list.len() as u8);
        for i in 0..4 {
            cpu.mem
                .wr8(0x8008_4598 + i, list.get(i as usize).copied().unwrap_or(0));
        }
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let retail = cpu.regs.r[2] as usize & 0xFFFF;

        let mut w = legaia_engine_core::world::World::new();
        let mut party = legaia_save::Party::zeroed(4);
        for (m, r) in party.members.iter_mut().zip(records.chunks(0x414)) {
            m.raw.copy_from_slice(r);
        }
        w.party.roster = party;
        w.party.party_actor_slots = list.iter().map(|&i| Some(i)).collect();
        w.party.field_list_emptied = list.is_empty();
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let port = match w.step_field() {
            Some(StepResult::Advance { next_pc }) => next_pc,
            other => panic!("{script:02x?}: {other:?}"),
        };
        ran += 1;
        if retail != script.len() - 1 {
            taken += 1;
        }
        if port != retail {
            *differ
                .entry(format!(
                    "{script:02x?} on {list:?}: retail {retail} port {port}"
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(20).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(
        ran == 4000 && taken > 500 && ran - taken > 500,
        "{ran} {taken}"
    );
}

/// Every opcode `0x21..=0x4F` with every first operand byte and generated
/// tails (zeros, `0xFF`s, small bytes, a per-byte mix, and a per-halfword mix
/// of `0` / `-1` / small / any; nine of the last for `43` and `4C`), in
/// [`corpus`]'s shape.
pub(crate) fn synthetic() -> Vec<(Vec<u8>, usize, String)> {
    let mut rng = crate::common::Rng(0x4F50_4E44);
    let mut out = Vec::new();
    for op in 0x21..=0x4Fu8 {
        // `43` and `4C` carry their sub-op in the first byte and several
        // operand words behind it: eight more per-word tails each.
        let tails = |b1: u8| match (op, b1) {
            // `43 09` tests one word for zero and three for `-1`.
            (0x43, 9) | (0x4C, 0xE4) => 69,
            (0x43 | 0x4C, _) => 13,
            _ => 5,
        };
        for (b1, tail) in (0..=0xFFu8).flat_map(|b| (0..tails(b)).map(move |t| (b, t))) {
            let mut script = vec![op, b1];
            // Zeros and `0xFF`s reach the arms that test an operand for
            // zero or `-1`, small bytes reach the rest, and a mix of the
            // three per byte reaches the arms that test one operand of
            // several.
            if tail < 4 {
                script.extend((0..62).map(|_| match tail {
                    0 => 0,
                    1 => 0xFF,
                    2 => rng.below(0x20) as u8,
                    _ => match rng.below(4) {
                        0 => 0,
                        1 => 0xFF,
                        2 => rng.below(0x20) as u8,
                        _ => rng.next() as u8,
                    },
                }));
            } else {
                // Per halfword: `0`, `-1`, small or any, at both byte
                // parities, so a word read at an odd offset is whole too.
                if tail % 2 == 1 {
                    script.push(rng.below(0x20) as u8);
                }
                while script.len() < 64 {
                    let w: u16 = match rng.below(4) {
                        0 => 0,
                        1 => 0xFFFF,
                        2 => rng.below(0x40) as u16,
                        _ => rng.next() as u16,
                    };
                    script.extend(w.to_le_bytes());
                }
                script.truncate(64);
            }
            // The zeroed RAM past the bytes, which an inline text walk
            // reads on retail's side.
            script.resize(0x200, 0);
            let label = if op == 0x4C || op == 0x43 {
                format!("{op:02X} {b1:02X}")
            } else {
                format!("{op:02X}")
            };
            out.push((script, 0, label));
        }
    }
    out
}

/// Every opcode with every first operand byte, the rest generated: the
/// sub-op values no script on the disc carries, which land on each switch's
/// default arm (`43` past sub `0x15`, `35` outside `1..=0xB`, `4E` past
/// `0xB`, `49` past `0xD`, `4C 1x` past `0x14`, `4C 4x` past `0xD`, `4C 5x`
/// past `4`, `4C EF`, `42` past mode `1`). Compared: the next PC, with
/// every call stubbed and every flag clear as in the disc corpus.
#[test]
fn every_operand_byte_steps_where_retail_steps() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = CTX);
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.hook(PACKET_LENGTH, |r, m| {
        let bytes = m.read(r.r[4], 0x400);
        r.r[2] = legaia_scene_formats::field_disasm::packet_length(&bytes) as u32;
    });
    cpu.step_budget = 20_000;
    cpu.mem.wr8(0x1F80_0393, 1);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let (mut ran, mut agree) = (0u32, 0u32);
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, _, _) in synthetic() {
        let (op, b1) = (script[0], script[1]);
        {
            cpu.mem = clean.clone();
            cpu.regs.r[GP] = gp;
            cpu.mem.load(SCRIPT, &script);
            cpu.mem.wr32(CTX + 0x94, SCRIPT);
            if cpu.call(VM, &[SCRIPT, 0, CTX]) != Stop::Returned {
                continue;
            }
            let retail = cpu.regs.r[2] as usize & 0xFFFF;
            let mut ctx = FieldCtx::default();
            let port = match step(&mut Inert { hit: true }, &mut ctx, &script, 0) {
                StepResult::Advance { next_pc } => next_pc,
                StepResult::Yield { resume_pc } => resume_pc,
                StepResult::Halt { final_pc } => final_pc,
                StepResult::Pending { .. } | StepResult::Unknown { .. } => continue,
            };
            ran += 1;
            if port & 0xFFFF == retail {
                agree += 1;
                continue;
            }
            let key = if op == 0x4C {
                format!("4C {b1:02X}")
            } else {
                format!("{op:02X}")
            };
            let e = differ.entry(key).or_default();
            e.0 += 1;
            if e.0 == 1 {
                e.1 = format!(
                    "retail {retail:#x} port {port:#x} bytes {:02x?}",
                    &script[..8]
                );
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(
        report.is_empty(),
        "{ran} ran {agree} agree\n{}",
        report.join("\n")
    );
    assert!(ran > 60_000, "{ran}");
}

/// `36`, the sound-cue op, and `35` sub `9` / `0xA`, the BGM swap barriers,
/// stepped by the real `World` on seeded audio words: the side-band pair
/// `0x8007BABC` / `0x8007BAA0`, the dual-mode word `0x8007B868`, the drive
/// word `0x8007BC20` (`36 FF FF` waits on it, `0x801E02E4..0x801E0308`) and
/// the swap walk's requested / loaded tracks `0x8007BAB8` / `0x8007BA9C` with
/// sound-flag bits 0 and 3 (`0x8007B750`), which `4C C9` also holds on. The
/// port keeps the walk as a state (`BgmSwapPoller`), seeded here as the
/// words describe it: idle
/// (tracks equal), loading (tracks differ) or settled (tracks differ, bits 0
/// and 3 up). Compared: the next PC and, for `36`, the request word.
#[test]
fn a_sound_cue_or_bgm_barrier_waits_as_retail_waits() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const REQ: u32 = 0x8007_BABC;
    const ACK: u32 = 0x8007_BAA0;
    const DUAL: u32 = 0x8007_B868;
    const DRIVE: u32 = 0x8007_BC20;
    const WANT: u32 = 0x8007_BAB8;
    const HAVE: u32 = 0x8007_BA9C;
    const SOUND: u32 = 0x8007_B750;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x5841_4655);
    let (mut waited, mut ran) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for round in 0..6000u32 {
        let pick = |rng: &mut crate::common::Rng| [-1, 3, 5, 8][rng.below(4) as usize];
        let (req, ack) = (pick(&mut rng), pick(&mut rng));
        let dual = if rng.below(4) == 0 { 1 } else { 0 };
        let drive = if rng.below(2) == 0 {
            0
        } else {
            1 + rng.below(20)
        };
        let walk = rng.below(3);
        let arg = rng.below(0x100) as u16;
        let script: Vec<u8> = if round % 4 == 3 {
            // `4C C9`, the hold on the same walk (`0x801E290C..0x801E292C`).
            vec![0x4C, 0xC9, 0x21]
        } else if round % 4 == 0 {
            // `35 <track> <sub>` for sub 9 and 0xA, under no dual-mode word:
            // the port's barriers do not read it.
            let sub = if rng.below(2) == 0 { 9 } else { 0xA };
            vec![0x35, arg as u8, 0, sub, 0x21]
        } else {
            let op0: u16 = match rng.below(5) {
                0 => 0xFFFF,
                1 => 0x8000 | rng.below(6) as u16,
                2 => 0,
                3 => rng.below(0x7FFF) as u16 + 1,
                _ => 0x8000 | rng.below(0x8000) as u16,
            };
            let [a, b] = op0.to_le_bytes();
            let [c, e] = arg.to_le_bytes();
            vec![0x36, a, b, c, e, 0x21]
        };
        let dual = if script[0] == 0x35 { 0 } else { dual };

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.wr32(REQ, req as u32);
        cpu.mem.wr32(ACK, ack as u32);
        cpu.mem.wr32(DUAL, dual);
        cpu.mem.wr32(DRIVE, drive);
        cpu.mem.wr32(WANT, 7);
        cpu.mem.wr32(HAVE, if walk == 0 { 7 } else { 6 });
        cpu.mem.wr32(SOUND, if walk == 2 { 9 } else { 0 });
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let retail = cpu.regs.r[2] as usize & 0xFFFF;
        let retail_req = cpu.mem.rd32(REQ) as i32;

        let mut w = legaia_engine_core::world::World::new();
        {
            let g = w.script_audio_gates_mut();
            g.sound_stream.requested = req;
            g.sound_stream.acked = ack;
            *g.dual_mode_gate = dual as i32;
            *g.xa_busy_frames = drive as u16;
            if walk != 0 {
                g.bgm_swap.start(0);
            }
            if walk == 2 {
                let _ = g.bgm_swap.tick(0x100, true, 0);
            }
        }
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let port = match w.step_field() {
            Some(StepResult::Advance { next_pc }) => next_pc,
            Some(StepResult::Halt { final_pc }) => final_pc,
            other => panic!("{script:02x?}: {other:?}"),
        };
        // Retail's `0xA` waits for bit 3 even with no walk in flight; the
        // port lets an idle walk through - a port gap, not compared.
        if script[0] == 0x35 && script[3] == 0xA && walk == 0 {
            continue;
        }
        ran += 1;
        if retail == 0 {
            waited += 1;
        }
        let port_req = w.script_audio_gates_mut().sound_stream.requested;
        if port != retail || (script[0] == 0x36 && port_req != retail_req) {
            *differ
                .entry(format!(
                    "{script:02x?} req {req} ack {ack} dual {dual} drive {drive} walk {walk}: \
                     retail pc {retail} req {retail_req}, port pc {port} req {port_req}"
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(20).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(waited > 500 && ran - waited > 500, "{ran} {waited}");
}

/// Every unprefixed instruction of the disc corpus and of [`synthetic`] with
/// the actor resolver `FUN_8003C83C` finding nothing: the arms that resolve
/// an operand-named actor themselves (`43` sub 2's three talkers, `4C 14`,
/// `4C 86`'s reflection source, `4C D2`, `4C E3`, `3E`'s warp target, `4C EB`)
/// take their miss branch on retail's side, and the port's host answers the
/// miss (`actor_guard` is `None`, the bag holds nothing). Compared: the next
/// PC. A prefixed op's own target is resolved by `World`'s routing before the
/// VM runs (`step_on_resolved_channel`), so the prelude's miss
/// (`0x801DE8A8..0x801DE8DC`) is not the VM's and prefixed ops are skipped.
#[test]
fn a_missing_actor_steps_where_retail_steps() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(RESOLVE_ACTOR, |r, _| r.r[2] = 0);
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.hook(PACKET_LENGTH, |r, m| {
        let bytes = m.read(r.r[4], 0x400);
        r.r[2] = legaia_scene_formats::field_disasm::packet_length(&bytes) as u32;
    });
    cpu.stub(0x8004_2EE0, 0x100);
    cpu.step_budget = 20_000;
    cpu.mem.wr8(0x1F80_0393, 1);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut ran = 0u32;
    let mut differ: BTreeMap<String, (u32, String)> = BTreeMap::new();
    for (script, pc, label) in corpus().into_iter().chain(synthetic()) {
        if script[pc] & 0x80 != 0 {
            continue;
        }
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(SCRIPT, &script);
        cpu.mem.wr32(CTX + 0x94, SCRIPT);
        if cpu.call(VM, &[SCRIPT, pc as u32, CTX]) != Stop::Returned {
            continue;
        }
        let retail = cpu.regs.r[2] as usize & 0xFFFF;
        let mut ctx = FieldCtx::default();
        let port = match step(&mut Inert { hit: false }, &mut ctx, &script, pc) {
            StepResult::Advance { next_pc } => next_pc,
            StepResult::Yield { resume_pc } => resume_pc,
            StepResult::Halt { final_pc } => final_pc,
            StepResult::Pending { .. } | StepResult::Unknown { .. } => continue,
        };
        ran += 1;
        if port & 0xFFFF != retail {
            let e = differ.entry(label).or_default();
            e.0 += 1;
            if e.0 == 1 {
                e.1 = format!(
                    "pc {pc:#x} retail {retail:#x} port {port:#x} bytes {:02x?}",
                    &script[pc..(pc + 10).min(script.len())]
                );
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{ran} ran\n{}", report.join("\n"));
    assert!(ran > 60_000, "{ran}");
}

/// `4C C1`, the fog-region reset, stepped by the real `World`: retail walks
/// the MAN section-4 records at `*0x80073ED8` (count `0x80073EDC`, stride
/// `0xB`) and sets each record's enable byte to whether its story flag (bytes
/// 9..10, through `FUN_8003CE64`) is clear (`0x801E2674..0x801E26EC`). The
/// port resets `World::fog.regions` from `World::system_flag_test`. The
/// flag test is answered on retail's side from the same generated bank the
/// world holds. Compared: every record's enable against the region's.
#[test]
fn a_fog_reset_enables_the_regions_retail_enables() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_render_kernels::fog_volume::FogRegion;
    const TABLE: u32 = 0x8013_0000;
    const BANK: u32 = 0x8013_1000;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    // `FUN_8003CE64`: `0xFF` when bit `idx` (MSB first) of the bank is up.
    cpu.hook(0x8003_CE64, |r, m| {
        let idx = r.r[4] & 0xFFFF;
        let set = m.rd8(BANK + (idx >> 3)) & (0x80 >> (idx & 7)) != 0;
        r.r[2] = if set { 0xFF } else { 0 };
    });
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x464F_4752);
    let (mut on, mut off) = (0u32, 0u32);
    for round in 0..500u32 {
        let n = rng.below(5) as usize;
        let null = rng.below(6) == 0;
        let bank: Vec<u8> = (0..8).map(|_| rng.next() as u8).collect();
        let mut table = vec![0u8; n * 0xB];
        for b in table.iter_mut() {
            *b = rng.next() as u8;
        }
        for r in table.chunks_mut(0xB) {
            r[9] = rng.below(64) as u8;
            r[10] = 0;
        }
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(TABLE, &table);
        cpu.mem.load(BANK, &bank);
        cpu.mem.wr32(0x8007_3ED8, if null { 0 } else { TABLE });
        cpu.mem.wr8(0x8007_3EDC, n as u8);
        cpu.mem.load(SCRIPT, &[0x4C, 0xC1, 0x21]);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(cpu.regs.r[2], 2);
        let after = cpu.mem.read(TABLE, n * 0xB);

        let mut w = legaia_engine_core::world::World::new();
        if !null {
            w.fog.regions = table.chunks(0xB).filter_map(FogRegion::parse).collect();
        }
        for idx in 0..64u16 {
            if bank[usize::from(idx >> 3)] & (0x80 >> (idx & 7)) != 0 {
                w.system_flag_set(idx);
            }
        }
        w.field_bytecode = vec![0x4C, 0xC1, 0x21];
        w.field_pc = 0;
        assert!(matches!(
            w.step_field(),
            Some(StepResult::Advance { next_pc: 2 })
        ));
        let retail: Vec<bool> = after.chunks(0xB).map(|r| r[0] != 0).collect();
        let port: Vec<bool> = if null {
            table.chunks(0xB).map(|r| r[0] != 0).collect()
        } else {
            w.fog.regions.iter().map(|r| r.enabled).collect()
        };
        assert_eq!(retail, port, "round {round}: {table:02x?}");
        on += retail.iter().filter(|&&e| e).count() as u32;
        off += retail.iter().filter(|&&e| !e).count() as u32;
    }
    assert!(on > 100 && off > 100, "{on} {off}");
}

/// `4C E5`, the casino coin bank's script delta, stepped by the real `World`
/// on a generated bank: retail sign-extends the 24-bit operand
/// (`0x801E3298..0x801E32A8`), adds it to `0x800845A4` and caps the sum at
/// 9 999 999 (`0x801E32C8..0x801E32D4`); the port is
/// `World::add_script_coins`. Compared: the bank. A sum below zero is left
/// out: retail keeps the negative word, the port's unsigned bank floors at
/// zero by decision (`casino_coin_bank`'s module docs).
#[test]
fn a_coin_delta_moves_the_bank_as_retail_moves_it() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const BANK: u32 = 0x8008_45A4;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    // The 24-bit operand read `FUN_8003CEB8`.
    cpu.hook(0x8003_CEB8, |r, m| {
        let a = r.r[4];
        r.r[2] = u32::from(m.rd8(a)) | u32::from(m.rd8(a + 1)) << 8 | u32::from(m.rd8(a + 2)) << 16;
    });
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x434F_494E);
    let (mut ran, mut capped, mut debits) = (0u32, 0u32, 0u32);
    for _ in 0..3000u32 {
        let bank = match rng.below(3) {
            0 => 9_999_999 - rng.below(5000),
            1 => rng.below(2000),
            _ => rng.below(10_000_000),
        };
        let delta = match rng.below(3) {
            0 => rng.below(0x100_0000),
            1 => 0x100_0000 - 1 - rng.below(5000),
            _ => rng.below(5000),
        };
        let [a, b, c, _] = delta.to_le_bytes();
        let script = [0x4C, 0xE5, a, b, c, 0x21];
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.wr32(BANK, bank);
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(cpu.regs.r[2], 5);
        let retail = cpu.mem.rd32(BANK) as i32;
        if retail < 0 {
            continue;
        }
        let mut w = legaia_engine_core::world::World::new();
        w.minigames.casino_coins = bank;
        w.field_bytecode = script.to_vec();
        w.field_pc = 0;
        assert!(matches!(
            w.step_field(),
            Some(StepResult::Advance { next_pc: 5 })
        ));
        assert_eq!(
            w.minigames.casino_coins, retail as u32,
            "bank {bank} delta {delta:#x}"
        );
        assert!(w.system_flag_test(8));
        ran += 1;
        capped += u32::from(retail == 9_999_999);
        debits += u32::from(delta & 0x80_0000 != 0);
    }
    assert!(
        ran > 1000 && capped > 100 && debits > 300,
        "{ran} {capped} {debits}"
    );
}

/// `4C E8`, the narration crawl's config op, stepped by the real `World`:
/// `w3 == 0` seeds the scene's crawl block `*0x801C6EA4 + 0x4C..0x50` (a zero
/// word takes its default); otherwise retail finds the crawl actor
/// (`FUN_8003CF04` over `*0x8007C34C` for handler `0x80037174`) and, with
/// one live, pauses it (`w3 == 1`, `w0 == 0`: `+0x10 |= 0x80000`), sets the
/// release count `+0x52` (`w3 == 1`, `w0 != 0`), resumes it (`w3 == 2`) or
/// retires it (`w3 == 3`: `+0x10 |= 8`) (`0x801E3378..0x801E34BC`). The port
/// keeps the block as `cutscene.narration_seed` and the actor as
/// `cutscene.narration`. The kill also frees the context the crawl's op
/// halt-acquired, the actor's `+0x94` when it is not null
/// (`0x801E344C..0x801E3468`); the port's link is the roller's parent, a
/// channel here. Compared: the block, the pause bit, whether the crawl lives
/// on and whether the parent still holds `0x400`.
#[test]
fn a_crawl_config_op_moves_the_crawl_as_retail_moves_it() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_field::cutscene_narration::{CutsceneNarration, RollerSeed};
    const SCENE: u32 = 0x8013_0000;
    const CRAWL: u32 = 0x8013_1000;
    const WAITER: u32 = 0x8013_2000;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    let live = std::rc::Rc::new(std::cell::Cell::new(false));
    let l = live.clone();
    cpu.hook(0x8003_CF04, move |r, _| {
        r.r[2] = if l.get() && r.r[5] == 0x8003_7174 {
            CRAWL
        } else {
            0
        };
    });
    cpu.mem.wr32(0x801C_6EA4, SCENE);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x4352_4157);
    let mut seen = std::collections::BTreeSet::new();
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for _ in 0..4000u32 {
        let word = |rng: &mut crate::common::Rng| match rng.below(3) {
            0 => 0i16,
            _ => rng.below(0x80) as i16 - 0x10,
        };
        let (w0, w1, w2) = (word(&mut rng), word(&mut rng), word(&mut rng));
        let w3 = rng.below(5) as i16;
        let seed = RollerSeed {
            top: rng.below(0x100) as i16,
            slots: rng.below(16) as i16,
            divisor: rng.below(8) as i16,
            release: rng.below(16) as i16,
        };
        let present = rng.below(4) != 0;
        let paused = rng.below(2) == 0;
        let mut script = vec![0x4C, 0xE8];
        for w in [w0, w1, w2, w3] {
            script.extend(w.to_le_bytes());
        }
        script.push(0x21);

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        for (o, v) in [
            (0x4C, seed.top),
            (0x4E, seed.slots),
            (0x50, seed.divisor),
            (0x52, seed.release),
        ] {
            cpu.mem.wr16(SCENE + o, v as u16);
        }
        live.set(present);
        cpu.mem
            .wr32(CRAWL + 0x10, if paused { 0x8_0000 } else { 0 });
        // The context the crawl releases on retirement, or none.
        let waiter = if rng.below(4) == 0 { 0 } else { WAITER };
        cpu.mem.wr32(CRAWL + 0x94, waiter);
        cpu.mem.wr32(WAITER + 0x10, 0x400);
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(cpu.regs.r[2], 10);
        let rd = |cpu: &mut psx_retail_exec::Cpu, o| cpu.mem.rd16(SCENE + o) as i16;
        let retail_seed = RollerSeed {
            top: rd(&mut cpu, 0x4C),
            slots: rd(&mut cpu, 0x4E),
            divisor: rd(&mut cpu, 0x50),
            release: rd(&mut cpu, 0x52),
        };
        let flags = cpu.mem.rd32(CRAWL + 0x10);
        let retail_crawl = present.then_some((flags & 0x8_0000 != 0, flags & 8 == 0));

        let mut w = legaia_engine_core::world::World::new();
        w.cutscene.narration_seed = seed;
        // The crawl's parent: a channel holding the halt bit, as the crawl
        // op left it.
        w.field_vm
            .channels
            .push(legaia_engine_field::field_channels::FieldChannel {
                placement_index: 5,
                ctx: legaia_engine_vm::field::FieldCtx {
                    script_id: 0x06,
                    flags: 0x400,
                    ..Default::default()
                },
                record_offset: 0,
                pc: 0,
                done: true,
                object_bind: false,
            });
        if present {
            let mut n = CutsceneNarration::with_seed(vec!["line".into()], seed, 1);
            if waiter != 0 {
                n = n.with_parent(0x06);
            }
            if paused {
                n.pause();
            }
            w.cutscene.narration = Some(n);
        }
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        assert!(matches!(
            w.step_field(),
            Some(StepResult::Advance { next_pc: 10 })
        ));
        let port_seed = w.cutscene.narration_seed;
        // A retired crawl is `None` on the port's side: (paused, lives).
        let port_crawl = present.then(|| {
            w.cutscene
                .narration
                .as_ref()
                .map_or((retail_crawl.unwrap().0, false), |n| (n.is_paused(), true))
        });
        // The parent's halt bit: freed by the kill alone, through the link.
        let retail_held = cpu.mem.rd32(WAITER + 0x10) & 0x400 != 0;
        let port_held = w.field_vm.channels[0].ctx.flags & 0x400 != 0;
        seen.insert((w3.min(4), present));
        if port_seed != retail_seed || port_crawl != retail_crawl || port_held != retail_held {
            *differ
                .entry(format!(
                    "w {w0} {w1} {w2} {w3} live {present} parent {waiter:#x}: retail \
                     {retail_seed:?} {retail_crawl:?} held {retail_held}, \
                     port {port_seed:?} {port_crawl:?} held {port_held}"
                ))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(seen.len() >= 10, "{seen:?}");
}

/// `34 0x`, the screen-effect colour tween, stepped by the real `World` on a
/// generated latch: retail retires a live tween (`*0x8007B62C`, `+0x10 |= 8`)
/// and spawns its walk-out, re-latches the blend `0x8007BCE0`, the kind
/// `0x8007BCCC` and the target `0x8007BCCF..CD` (r first) from the operands,
/// and then clears `0x8007B62C` for an all-zero target or spawns the walk-in
/// through `FUN_801DE2B0` and parks it there (`0x801DFCAC..0x801DFEF8`). The port is
/// `set_effect_colour` over `World::presentation`. Compared: the old tween's
/// retire bit, the blend, the kind, the target and whether a tween is parked.
/// The spawns' own ramps are not.
#[test]
fn an_effect_colour_retires_and_latches_as_retail_does() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    use legaia_engine_core::world::field_test_drivers::FieldTestDrivers;
    use legaia_engine_field::actor_colour_tween::tween_from_fade_template;
    use legaia_engine_system::fade::FadeTemplate;
    const PARKED: u32 = 0x8007_B62C;
    const OLD: u32 = 0x8013_0000;
    const NEW: u32 = 0x8013_1000;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    // The tween spawner: a fresh actor.
    cpu.hook(0x801D_E2B0, |r, _| r.r[2] = NEW);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x4546_4645);
    let (mut retired, mut cleared, mut parked) = (0u32, 0u32, 0u32);
    for round in 0..3000u32 {
        let live = rng.below(2) == 0;
        let old_rgb = [
            rng.below(256) as u8,
            rng.below(256) as u8,
            rng.below(256) as u8,
        ];
        let (old_blend, old_kind) = (
            [1u8, 2][rng.below(2) as usize],
            [0u8, 2, 8][rng.below(3) as usize],
        );
        let op0 = rng.below(8) as u8;
        let rgb = match rng.below(4) {
            0 => [0, 0, 0],
            1 => [0xFF, 0xFF, 0xFF],
            _ => [
                rng.below(256) as u8,
                rng.below(256) as u8,
                rng.below(256) as u8,
            ],
        };
        let dur = (rng.below(0x80) as i16).to_le_bytes();
        let script = [0x34, op0, rgb[0], rgb[1], rgb[2], dur[0], dur[1], 0x21];

        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.wr32(PARKED, if live { OLD } else { 0 });
        cpu.mem.wr32(OLD + 0x10, 0);
        cpu.mem.wr8(0x8007_BCE0, old_blend);
        cpu.mem.wr8(0x8007_BCCC, old_kind);
        // The latch is stored blue first: r at `0x8007BCCF`, b at `BCCD`.
        cpu.mem
            .load(0x8007_BCCD, &[old_rgb[2], old_rgb[1], old_rgb[0]]);
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        assert_eq!(cpu.regs.r[2], 7);
        let retail = (
            live && cpu.mem.rd32(OLD + 0x10) & 8 != 0,
            i16::from(cpu.mem.rd8(0x8007_BCE0)),
            i16::from(cpu.mem.rd8(0x8007_BCCC)),
            cpu.mem
                .read(0x8007_BCCD, 3)
                .iter()
                .rev()
                .map(|&b| i16::from(b))
                .collect::<Vec<_>>(),
            cpu.mem.rd32(PARKED) != 0,
        );

        let mut w = legaia_engine_core::world::World::new();
        w.presentation.effect_blend = i16::from(old_blend);
        w.presentation.effect_kind = i16::from(old_kind);
        w.seed_effect_target_rgb(old_rgb.map(i16::from));
        let old = live.then(|| {
            let tween = tween_from_fade_template(
                &FadeTemplate {
                    kind: i16::from(old_blend),
                    duration: 8,
                    start_rgb: [0; 3],
                    end_rgb: old_rgb.map(i16::from),
                    mode: [0, -1, 0],
                },
                i16::from(old_kind),
            );
            w.spawn_colour_tween(tween).expect("a free slot")
        });
        w.presentation.effect_tween_slot = old;
        w.field_bytecode = script.to_vec();
        w.field_pc = 0;
        assert!(matches!(
            w.step_field(),
            Some(StepResult::Advance { next_pc: 7 })
        ));
        let port = (
            old.is_some_and(|s| w.actors[s].physics.status_flags & 8 != 0),
            w.presentation.effect_blend,
            w.presentation.effect_kind,
            w.effect_target_rgb().to_vec(),
            w.presentation.effect_tween_slot.is_some(),
        );
        assert_eq!(port, retail, "round {round}: {script:02x?} live {live}");
        retired += u32::from(retail.0);
        cleared += u32::from(rgb == [0, 0, 0]);
        parked += u32::from(retail.4);
    }
    assert!(
        retired > 500 && cleared > 300 && parked > 1000,
        "{retired} {cleared} {parked}"
    );
}

/// `4E` sub-ops `0` / `1` / `2`, the party-record compares, stepped by the
/// real `World` on generated records: retail scales the operand by the
/// record's `(+0x104, +0x106)` / `(+0x108, +0x10A)` pair (`factor * arg >> 8`,
/// rounded toward zero, `0x801E0A40..0x801E0ABC`) or reads the level byte
/// `+0x130` (`0x801E0AC0`), and compares signed either way round
/// (`0x801E0B88..0x801E0BB8`). The port reads the pair and the level off
/// `World`'s roster. Compared: the next PC.
#[test]
fn a_party_record_compare_skips_as_retail_skips() {
    differential!(prot(897, VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const RECORDS: u32 = 0x8008_4708;
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    cpu.hook(READ_U16, |r, m| r.r[2] = m.rd16(r.r[4]) as i16 as u32);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(CTX + 0x94, SCRIPT);
    let gp = cpu.regs.r[GP];
    let clean = cpu.mem.clone();
    let mut rng = crate::common::Rng(0x5245_434D);
    let (mut ran, mut taken) = (0u32, 0u32);
    let mut differ: BTreeMap<String, u32> = BTreeMap::new();
    for _ in 0..6000u32 {
        let mut records = vec![0u8; 4 * 0x414];
        for b in records.iter_mut() {
            *b = rng.next() as u8;
        }
        for r in records.chunks_mut(0x414) {
            for o in [0x104, 0x106, 0x108, 0x10A] {
                let v = rng.below(0x400) as u16;
                r[o..o + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        let page = rng.below(4) as u8;
        let mode = (rng.below(3) as u8) << 4 | rng.below(3) as u8;
        let arg: i16 = match rng.below(3) {
            0 => -(rng.below(0x400) as i16),
            1 => rng.below(0x400) as i16,
            _ => rng.next() as i16,
        };
        let [a, b] = arg.to_le_bytes();
        let script = vec![0x4E, page, mode, a, b, 0x10, 0x00, 0x21];
        cpu.mem = clean.clone();
        cpu.regs.r[GP] = gp;
        cpu.mem.load(RECORDS, &records);
        cpu.mem.load(SCRIPT, &script);
        assert_eq!(cpu.call(VM, &[SCRIPT, 0, CTX]), Stop::Returned);
        let retail = cpu.regs.r[2] as usize & 0xFFFF;

        let mut w = legaia_engine_core::world::World::new();
        let mut party = legaia_save::Party::zeroed(4);
        for (m, r) in party.members.iter_mut().zip(records.chunks(0x414)) {
            m.raw.copy_from_slice(r);
        }
        w.party.roster = party;
        w.field_bytecode = script.clone();
        w.field_pc = 0;
        let port = match w.step_field() {
            Some(StepResult::Advance { next_pc }) => next_pc,
            Some(StepResult::Halt { final_pc }) => final_pc,
            other => panic!("{script:02x?}: {other:?}"),
        };
        ran += 1;
        taken += u32::from(retail != 7);
        if port != retail {
            *differ
                .entry(format!("{script:02x?}: retail {retail} port {port}"))
                .or_default() += 1;
        }
    }
    let report: Vec<String> = differ.keys().take(12).cloned().collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    assert!(taken > 1000 && ran - taken > 1000, "{ran} {taken}");
}
