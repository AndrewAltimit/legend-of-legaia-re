//! Disc-gated: which **scripted-motion VM** opcodes the disc actually
//! authors, per scene MAN.
//!
//! The port of `FUN_80038158` runs the whole 32-slot op table
//! (`legaia_engine_vm::ambient_motion_ops`), but four of its arms reach no
//! consumer on either host - `0x0E` (model re-bind), `0x13` (`MoveImage`),
//! `0x15` / `0x16` (the pitch / roll tweens). Wiring a consumer for an arm no
//! authored stream ever executes is speculation dressed as parity, so this
//! census answers the prior question from the bytes: does the disc carry the
//! op at all, and in which scenes?
//!
//! It walks every CDNAME scene's MAN tail-**section 1** streams (the carrier
//! `legaia_asset::man_motion` decodes, every variant of every record) and
//! counts each opcode by the section's own width table. Printed as a table
//! with `--nocapture`; the assertions pin the arms whose absence or presence
//! a wiring decision rests on.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::scene_man_carriers;
use legaia_engine_core::scene::{ProtIndex, Scene};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// `op -> (total sites, scenes carrying it)`.
type Census = BTreeMap<u8, (usize, BTreeSet<String>)>;

/// How the walk left each variant - the honesty check on a zero count. A
/// census that stops early on an unknown byte under-reports every op after
/// it, so the two tallies are printed beside the table.
#[derive(Default)]
struct WalkStats {
    variants: usize,
    ran_to_end: usize,
    stopped_on_unknown_op: usize,
}

fn census() -> Option<(Census, WalkStats)> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut out: Census = BTreeMap::new();
    let mut stats = WalkStats::default();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for rec in legaia_asset::man_motion::motion_records(man, &man_file) {
                for var in legaia_asset::man_motion::stream_variants(man, &rec) {
                    stats.variants += 1;
                    let mut pc = var.code_offset;
                    let mut clean = true;
                    while pc < var.code_end && pc < man.len() {
                        let op = man[pc];
                        let Some(w) = legaia_asset::man_motion::op_width(op) else {
                            clean = false;
                            break;
                        };
                        let e = out.entry(op).or_default();
                        e.0 += 1;
                        e.1.insert(name.clone());
                        pc += w;
                    }
                    if clean {
                        stats.ran_to_end += 1;
                    } else {
                        stats.stopped_on_unknown_op += 1;
                    }
                }
            }
        }
    }
    Some((out, stats))
}

/// What op `0x0E`'s operands actually name, per scene, against **the scene's
/// model bank** - the pool ids `DAT_8007C018[5..]` that
/// [`legaia_engine_core::model_bank::SceneModelBank`] reconstructs in retail's
/// own registration order - and against the set of models that scene's
/// placements already bind.
///
/// The count above says how much op `0x0E` is authored; this says what
/// implementing it would cost. Both hosts resolve an NPC's mesh from
/// `placement.model_index` at spawn - the native window uploads one GPU mesh
/// per placement in `upload_assets`, the play page builds catalog entry `i`'s
/// mesh in `play_npc_mesh` - so a swap target that is *also* some placement's
/// spawn model is already resident and a swap target that is not has no mesh
/// anywhere on either host. That split is the denominator for the
/// "per-placement mesh re-bind" gap in `docs/tooling/host-drift.md`.
///
/// The **resolvability** half is settled: every operand resolves. An earlier
/// version of this test measured the operand against
/// `SceneResources::tmds.len()`, which is a magic scan over the scene's raw
/// entries and so is blind to a TMD inside an LZS-compressed bundle
/// descriptor - it reported banks of 1 and 0 for `koin3` / `other7`, where the
/// registration order holds 77 and 65.
#[test]
fn model_swap_operands_against_the_placement_model_set_or_skip() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut total_sites = 0usize;
    let mut total_resident = 0usize;
    let mut total_unresolved = 0usize;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        // Every model id some placement in this scene binds at spawn. Kept
        // as the RAW id on both sides - `PlacementRecord::special_model` is
        // `model_index >= 0xF0` and leaves `model_index` raw, so folding the
        // swap operand's `- 0xF0` in here would compare two different
        // numbers and report a spurious zero overlap.
        let mut spawn_models: BTreeSet<i16> = BTreeSet::new();
        let mut swaps: BTreeMap<i16, usize> = BTreeMap::new();
        let mut carriers: BTreeSet<(u32, Option<usize>)> = BTreeSet::new();
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for (p, _kind) in
                legaia_engine_core::man_field_scripts::classify_placements(&man_file, man)
            {
                spawn_models.insert(i16::from(p.model_index));
            }
            for rec in legaia_asset::man_motion::motion_records(man, &man_file) {
                for var in legaia_asset::man_motion::stream_variants(man, &rec) {
                    let mut pc = var.code_offset;
                    while pc < var.code_end && pc < man.len() {
                        let op = man[pc];
                        let Some(w) = legaia_asset::man_motion::op_width(op) else {
                            break;
                        };
                        if op == 0x0E {
                            carriers.insert((carrier.entry_idx, carrier.chunk_offset));
                        }
                        if op == 0x0E && pc + 2 < man.len() {
                            let id = i16::from_le_bytes([man[pc + 1], man[pc + 2]]);
                            *swaps.entry(id).or_default() += 1;
                        }
                        pc += w;
                    }
                }
            }
        }
        if swaps.is_empty() {
            continue;
        }
        let sites: usize = swaps.values().sum();
        let resident: usize = swaps
            .iter()
            .filter(|(k, _)| spawn_models.contains(k))
            .map(|(_, n)| *n)
            .sum();
        let targets: Vec<i16> = swaps.keys().copied().collect();
        // Does the target resolve in the scene's model bank? That is the
        // second half of the cost: a target the bank does not hold could not
        // be materialised at all.
        let bank = legaia_engine_core::model_bank::SceneModelBank::build(&scene);
        let resolved = targets
            .iter()
            .filter(|&&id| bank.source_for_model_id(id).is_some())
            .count();
        let player_bank = targets
            .iter()
            .filter(|&&id| {
                legaia_engine_core::model_bank::resolve_model_id(id).bank
                    == legaia_engine_core::model_bank::ModelBank::Player
            })
            .count();
        total_sites += sites;
        total_resident += resident;
        total_unresolved += targets
            .iter()
            .filter(|&&id| {
                bank.source_for_model_id(id).is_none()
                    && legaia_engine_core::model_bank::resolve_model_id(id).bank
                        != legaia_engine_core::model_bank::ModelBank::Player
            })
            .map(|id| swaps.get(id).copied().unwrap_or(0))
            .sum::<usize>();
        eprintln!(
            "[model swap] {name}: {sites} sites, {} targets {targets:?}; \
             {resident} hit a spawn model ({} of {} placement models); \
             scene model bank {}, {resolved} targets resolve in it, \
             {player_bank} name the player bank; \
             carriers {carriers:?}",
            swaps.len(),
            swaps.keys().filter(|k| spawn_models.contains(k)).count(),
            spawn_models.len(),
            bank.len(),
        );
    }
    eprintln!(
        "[model swap] total {total_sites} sites, {total_resident} already-resident, \
         {} needing a model no placement binds, {total_unresolved} unresolved",
        total_sites - total_resident
    );
    // Non-vacuous: the sibling census counts the same sites, so a zero here
    // means this walk is broken rather than that the disc authors no swap.
    assert!(total_sites > 0, "no op 0x0E operand decoded");
    // The finding the disclosure rests on, pinned so it cannot rot silently:
    // NO swap target is a model some placement in the same scene binds at
    // spawn, so the mechanism both hosts use to get an NPC a mesh reaches
    // none of these. If this ever goes non-zero, re-measure the gap in
    // `docs/tooling/host-drift.md` before quoting it again.
    assert_eq!(
        total_resident, 0,
        "a swap target is now a spawn model - re-measure the host-drift gap"
    );
    // The other half, and the one that was measured wrongly before: every
    // authored operand resolves to a model source. If this goes non-zero the
    // bank walk has lost a carrier, not the disc gained an unresolvable id.
    assert_eq!(
        total_unresolved, 0,
        "an op 0x0E operand no longer resolves in its scene's model bank"
    );
}

#[test]
fn scripted_motion_op_census_or_skip() {
    let Some((c, stats)) = census() else { return };
    eprintln!(
        "[motion ops] {} variants walked: {} ran to the record end, {} stopped on a byte the width table does not name",
        stats.variants, stats.ran_to_end, stats.stopped_on_unknown_op
    );
    eprintln!("[motion ops] op   sites  scenes");
    for (op, (sites, scenes)) in &c {
        eprintln!("[motion ops] {op:#04x} {sites:6} {:6}", scenes.len());
    }
    let count = |op: u8| c.get(&op).map(|e| e.0).unwrap_or(0);
    let scenes = |op: u8| c.get(&op).map(|e| e.1.len()).unwrap_or(0);
    // The census must have found the corpus at all: op `0x08` (system-flag
    // clear) is the arm the sibling `motion_flag_census_disc` pins by scene,
    // so a zero here means this walk is broken, not that the disc is empty.
    assert!(count(0x08) > 0, "motion-VM flag-clear arm found nowhere");
    // The four consumer-gap arms. Whatever these numbers are, they are the
    // denominator any "wire it on both hosts" decision has to quote.
    for op in [0x0E, 0x13, 0x15, 0x16] {
        let names: Vec<&str> = c
            .get(&op)
            .map(|e| e.1.iter().map(String::as_str).collect())
            .unwrap_or_default();
        eprintln!(
            "[motion ops] consumer-gap op {op:#04x}: {} sites over {} scenes {names:?}",
            count(op),
            scenes(op)
        );
    }
}
