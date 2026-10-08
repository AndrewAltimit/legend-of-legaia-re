//! Disc-gated census: which `+0x52` control words shipped move programs write
//! through move-VM op `0x15` (`Ctrl52`), and in particular whether any keeps
//! the camera **yaw** factor on a camera-relative node.
//!
//! `FUN_8001CF50` rebuilds the camera rotation for a node whose `+0x52`
//! carries a bit of `0x780`, leaving out each flagged axis (`0x80` pitch,
//! `0x100` yaw, `0x200` roll); `0x400` takes the saved-matrix arm first. So
//! the yaw factor `FUN_8004629C` (`GteMat3::rot_y` under
//! `legaia_engine_ui::gte::camera_view_rotation`) runs only for a word that
//! sets `0x80` or `0x200` and clears both `0x100` and `0x400`. The retail
//! captures (`docs/subsystems/renderer.md`) show `0x380`, `0x100` / `0x180`
//! and `0x400` only; this asks the disc the same question.
//!
//! Carriers walked, each through `slot_b_module::move_program_visit` (the
//! static width-sum walk the slot-B record bounds use, so a hit is an
//! instruction boundary, never a coincidental word pair):
//!
//! * every CDNAME scene's prescript stager records (`move_stager_records`);
//! * every slot-B cast / summon image's spawn records, `0903..=0966`.
//!
//! The test prints the operand histogram, asserts that the walk decoded op
//! `0x15` at all (so a zero is a real zero), and pins the one word that keeps
//! the yaw factor: `urudre1` stager record 14 writes `0x0080` on the taken
//! side of an ext `0x37` pool-headroom branch, which jumps the `HALT` the
//! fall-through ends on. A static walk that stopped at the first `HALT` never
//! saw it, which is how `docs/tooling/reach-triage.md` came to call
//! `8004629c` content-gated; any further carrier fails here.
//!
//! Not walked: the effect bundle's scripts and the scene bundles' type-`0x05`
//! MOVE payloads, whose program starts this census has no parser for.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_asset::scene_event_scripts::move_stager_records;
use legaia_asset::slot_b_module;
use legaia_engine_core::scene::SceneHost;

/// Move-VM opcode `0x15` - `+0x52 = operand`.
const OP_CTRL52: u16 = 0x15;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("CDNAME.TXT").exists()
    {
        return Some(d);
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

/// `true` when `FUN_8001CF50` would reach its yaw factor for this word.
fn keeps_yaw(word: u16) -> bool {
    word & 0x400 == 0 && word & 0x280 != 0 && word & 0x100 == 0
}

/// Every op-`0x15` operand on the static walk of the program at `start`.
fn ctrl52_operands(bytes: &[u8], start: usize) -> Vec<u16> {
    let mut out = Vec::new();
    slot_b_module::move_program_visit(bytes, start, |pc, op| {
        if op == OP_CTRL52
            && let Some(w) = bytes.get(pc + 2..pc + 4)
        {
            out.push(u16::from_le_bytes([w[0], w[1]]));
        }
    });
    out
}

#[test]
fn census_move_op_15_control_words() {
    let Some(root) = extracted_root() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&root.join("CDNAME.TXT")).expect("cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut hist = BTreeMap::<u16, usize>::new();
    let mut keep_yaw = Vec::new();
    let mut programs = 0usize;
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let scene = host.scene.as_ref().expect("scene loaded");
        let Some(scripts) = scene.find_event_scripts() else {
            continue;
        };
        let Some(records) = move_stager_records(scripts.bytes) else {
            continue;
        };
        for (id, rec) in records.iter().enumerate() {
            let bytes = &scripts.bytes[..rec.bytecode.end];
            programs += 1;
            for w in ctrl52_operands(bytes, rec.bytecode.start) {
                *hist.entry(w).or_default() += 1;
                if keeps_yaw(w) {
                    keep_yaw.push(format!("{name} stager record {id}: {w:#06x}"));
                }
            }
        }
    }

    let prot = root.join("PROT");
    for idx in 903u32..=966 {
        let Some(path) = std::fs::read_dir(&prot).ok().and_then(|rd| {
            rd.filter_map(Result::ok).map(|e| e.path()).find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&format!("{idx:04}_")))
            })
        }) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let layout = slot_b_module::parse(&bytes);
        for rec in layout.records.iter().chain(&layout.chained_records) {
            programs += 1;
            for w in ctrl52_operands(&bytes, rec.start + 4) {
                *hist.entry(w).or_default() += 1;
                if keeps_yaw(w) {
                    keep_yaw.push(format!("PROT {idx:04} record @{:#x}: {w:#06x}", rec.start));
                }
            }
        }
    }

    eprintln!("  {programs} move programs walked");
    eprintln!("  op 0x15 operand histogram: {hist:#06x?}");
    for line in &keep_yaw {
        eprintln!("  keeps yaw: {line}");
    }
    eprintln!(
        "[ok] {} op-0x15 sites, {} keep the yaw factor",
        hist.values().sum::<usize>(),
        keep_yaw.len()
    );
    assert!(
        !hist.is_empty(),
        "the walk decoded no op 0x15 at all - the census would be vacuous"
    );
    assert_eq!(
        keep_yaw,
        vec!["urudre1 stager record 14: 0x0080".to_string()],
        "the yaw-keeping carriers changed: reach-triage's 8004629c row names exactly \
         urudre1 stager record 14"
    );
}
