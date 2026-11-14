//! Disc-gated census of every field-VM record that stages a camera shot
//! (op `0x45` CONFIGURE), and of how each one hands the camera back.
//!
//! A staged shot is released in retail by one of four things: an op-`0x45`
//! APPLY (`FUN_801DAB90` composes the follow pose over the staging block and
//! `FUN_801DE084` copies or glides it in), a `[4C 39]` / `[4C 3E]` snap
//! (`FUN_801DB8EC`), a scene change or a battle (both re-run the field init
//! `FUN_80025C24`), or - with none of those - the follow ease `FUN_801DB510`,
//! which composes and eases toward the follow pose on every pass the player's
//! own tick moves him, whatever the staging block holds.
//!
//! The walk is a control-flow walk over the opcode-aware disassembler
//! (`legaia_scene_formats::field_disasm`), which steps text segments and
//! picker tables as strides of the stream, so the shots a talk stages behind
//! its first box are reached. For each reached CONFIGURE it asks whether a
//! slice end (`0x21`), the record's end, or an undecodable byte is reachable
//! from it without passing a release - the "staged and never applied" shape.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_field::man_field_scripts::partition_record_span;
use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_disasm::{CameraKind, Insn, InsnInfo, decode};
use legaia_scene_formats::man_section;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const PC_BUDGET: usize = 20_000;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// What one decoded instruction means for a staged shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Configure,
    Apply,
    /// `[4C 39]` / `[4C 3E]`: the snap `FUN_801DB8EC`.
    Snap,
    /// `0x3F` named scene change, a door warp or a scripted battle.
    Leave,
    /// `0x21`: the slice (or, after a box, the talk) ends here.
    SliceEnd,
    Other,
}

fn role(insn: &Insn) -> Role {
    match &insn.info {
        InsnInfo::Camera {
            kind: CameraKind::Configure { .. },
            ..
        } => Role::Configure,
        InsnInfo::Camera {
            kind: CameraKind::Apply { .. },
            ..
        } => Role::Apply,
        InsnInfo::MenuCtrl {
            op0: 0x39 | 0x3E, ..
        } => Role::Snap,
        InsnInfo::SceneChange { .. } | InsnInfo::WarpOrInteract { .. } => Role::Leave,
        InsnInfo::Nop if insn.opcode == 0x21 => Role::SliceEnd,
        _ => Role::Other,
    }
}

/// Control-flow successors of `insn` (absolute body offsets).
fn successors(insn: &Insn) -> Vec<usize> {
    let fall = insn.pc + insn.size.max(1);
    match &insn.info {
        InsnInfo::JmpRel { target, .. } => vec![*target],
        InsnInfo::CondJmp { target, .. } => vec![fall, *target],
        InsnInfo::BBoxTest { skip_target, .. } => vec![fall, *skip_target],
        InsnInfo::SystemFlag {
            target: Some(t), ..
        } => vec![fall, *t],
        InsnInfo::Picker { count, targets, .. } => targets[..(*count as usize).min(4)].to_vec(),
        InsnInfo::Byte { .. } => Vec::new(),
        _ => vec![fall],
    }
}

/// Decoded instructions reachable from `entry`.
fn reach(body: &[u8], entry: usize) -> BTreeMap<usize, Insn> {
    let mut out = BTreeMap::new();
    let mut work = vec![entry];
    while let Some(pc) = work.pop() {
        if pc >= body.len() || out.contains_key(&pc) || out.len() > PC_BUDGET {
            continue;
        }
        let Ok(insn) = decode(body, pc) else { continue };
        for s in successors(&insn) {
            work.push(s);
        }
        out.insert(pc, insn);
    }
    out
}

/// Whether a path from the CONFIGURE at `from` reaches a slice end, the end of
/// the body or a dead byte without a release on the way.
fn unreleased_path(body: &[u8], code: &BTreeMap<usize, Insn>, from: usize) -> bool {
    let mut seen = BTreeSet::new();
    let mut work: Vec<usize> = successors(&code[&from]);
    while let Some(pc) = work.pop() {
        if !seen.insert(pc) {
            continue;
        }
        let Some(insn) = code.get(&pc) else {
            // Past the body or undecodable: the record stops here.
            return true;
        };
        match role(insn) {
            Role::Apply | Role::Snap | Role::Leave => continue,
            Role::SliceEnd => return true,
            _ => {}
        }
        let succ = successors(insn);
        if succ.is_empty() || succ.iter().any(|s| *s >= body.len()) {
            return true;
        }
        work.extend(succ);
    }
    false
}

#[derive(Debug)]
struct Row {
    scene: String,
    entry: u32,
    partition: usize,
    record: usize,
    configures: Vec<(usize, u16, u16)>,
    applies: usize,
    snaps: usize,
    leaves: usize,
    texts: bool,
    /// CONFIGUREs with a release-free path to a slice end.
    open: Vec<usize>,
}

fn scene_names(extracted: &std::path::Path) -> Vec<String> {
    let Ok(cdname) = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn field_camera_shot_census() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Ok(index) = ProtIndex::open_extracted(&extracted) else {
        eprintln!("[skip] PROT index would not open");
        return;
    };
    let mut rows = Vec::new();
    let mut seen_carriers = BTreeSet::new();
    for name in scene_names(&extracted) {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            if !seen_carriers.insert(carrier.entry_idx) {
                continue;
            }
            let man = &carrier.payload;
            let Ok(man_file) = man_section::parse(man) else {
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
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let Some(body) = man.get(start..start + len) else {
                        continue;
                    };
                    let code = reach(body, pc0);
                    let mut row = Row {
                        scene: name.clone(),
                        entry: carrier.entry_idx,
                        partition,
                        record,
                        configures: Vec::new(),
                        applies: 0,
                        snaps: 0,
                        leaves: 0,
                        texts: false,
                        open: Vec::new(),
                    };
                    for (pc, insn) in &code {
                        if matches!(insn.info, InsnInfo::TextSegment { .. }) {
                            row.texts = true;
                        }
                        match role(insn) {
                            Role::Configure => {
                                if let InsnInfo::Camera {
                                    kind:
                                        CameraKind::Configure {
                                            mask,
                                            apply_trigger,
                                        },
                                    ..
                                } = insn.info
                                {
                                    row.configures.push((*pc, mask, apply_trigger));
                                }
                                if unreleased_path(body, &code, *pc) {
                                    row.open.push(*pc);
                                }
                            }
                            Role::Apply => row.applies += 1,
                            Role::Snap => row.snaps += 1,
                            Role::Leave => row.leaves += 1,
                            _ => {}
                        }
                    }
                    if !row.configures.is_empty() {
                        rows.push(row);
                    }
                }
            }
        }
    }
    let mut open = 0;
    for r in &rows {
        if !r.open.is_empty() {
            open += 1;
        }
        eprintln!(
            "SHOT {}\t{}\tP{}[{}]\tcfg={}\tapply={}\tsnap={}\tleave={}\ttext={}\topen={:?}\tcfgs={:?}",
            r.scene,
            r.entry,
            r.partition,
            r.record,
            r.configures.len(),
            r.applies,
            r.snaps,
            r.leaves,
            r.texts,
            r.open
                .iter()
                .map(|p| format!("0x{p:04X}"))
                .collect::<Vec<_>>(),
            r.configures
                .iter()
                .map(|(p, m, t)| format!("0x{p:04X}:m{m:03X}:t{t}"))
                .collect::<Vec<_>>(),
        );
    }
    eprintln!(
        "[census] {} shot records, {} with an unreleased path",
        rows.len(),
        open
    );
    assert!(rows.len() > 50, "the walk is not reaching the corpus");
    // The release-free shape exists on the disc, in a talk and in a door:
    // `koin4` P1[1] glides to a shot, waits on it and ends on `21`; `bubu1`
    // P1[22] stages five shots and no APPLY; `balden` P0[7] snaps the
    // arrival shot after a fade and loops without one. Each needs the
    // follow ease to take the camera back (`Camera::release_unheld_shot`).
    for (scene, partition, record) in [("koin4", 1, 1), ("bubu1", 1, 22), ("balden", 0, 7)] {
        let row = rows
            .iter()
            .find(|r| r.scene == scene && r.partition == partition && r.record == record)
            .unwrap_or_else(|| panic!("{scene} P{partition}[{record}] stages a shot"));
        assert!(
            !row.open.is_empty() && row.applies == 0 && row.snaps == 0 && row.leaves == 0,
            "{scene} P{partition}[{record}] ends on an unreleased shot: {row:?}"
        );
    }
}
