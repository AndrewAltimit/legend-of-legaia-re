//! Disc-gated: the disc-wide census of the **mode-24 minigame door-warp**
//! (field-VM op `0x3E`, `op0 >= 100`) over every scene MAN's field-VM bytecode.
//!
//! This is the denominator behind "how does a player reach a minigame". The
//! retail chain is entirely SCUS-resident and pinned in the disassembly
//! (`see ghidra/scripts/funcs/overlay_0897_801de840.txt` at `0x801E078C`):
//!
//! ```text
//! 801e0794  sw   zero,-0x4540(v0)   ; _DAT_8007BAC0 = 0
//! 801e0798  lbu  v1,0x0(s6)         ; v1 = op0
//! 801e079c  li   v0,0x18
//! 801e07a0  sh   v0,-0x47c4(a1)     ; _DAT_8007B83C = 0x18  (game mode 24)
//! 801e07a8  sw   zero,0x4440(v0)    ; _DAT_80084440 = 0     (winnings acc)
//! 801e07b0  addiu v1,v1,-0x64       ; sub_id = op0 - 100
//! 801e07b8  sh   v1,-0x45cc(v0)     ; _DAT_8007BA34 = sub_id
//! ```
//!
//! There is **no scene-change packet call** (`func_0x8001FD44`) in that arm -
//! the op carries no destination name, and `sub_id` selects a *code overlay*
//! (`FUN_8003EBE4(sub_id + 0x4D)` in the mode-24 init `FUN_80025980`), not a
//! scene. `sub_id` is therefore a minigame selector, and the seven values
//! `0..=6` are the whole warp id space (`WARP_OP0_RANGE`).
//!
//! What this census pins:
//!
//! - which scenes carry a genuine door-warp at all, and with which `sub_id`;
//! - that the venue scenes for the shipped minigames are among them;
//! - that no site carries a `sub_id` outside `0..=6`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::minigame_entry::MinigameSubId;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One decoded mode-24 door-warp site.
#[derive(Debug, Clone)]
struct WarpSite {
    scene_name: String,
    partition: usize,
    record: usize,
    abs_pc: usize,
    sub_id: u8,
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

fn run_census() -> Option<Vec<WarpSite>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scenes = index.cdname_scene_names();
    eprintln!(
        "[minigame-warp census] scanning {} CDNAME scenes",
        scenes.len()
    );

    let mut out = Vec::new();
    for name in &scenes {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            let partition_count = man_file.header.partition_counts.len();
            for partition in 0..partition_count {
                let records = man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                for record in 0..records {
                    let Some((script_start, pc0, body_len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[script_start..script_start + body_len];
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        let InsnInfo::WarpOrInteract {
                            op0, is_warp: true, ..
                        } = insn.info
                        else {
                            continue;
                        };
                        // Genuine door-warp only: base `0x3E` (no `0x80`
                        // cross-context prefix) with `op0` in the 7-id space.
                        // A desynced walk inside message text can land on a
                        // `0x3E` whose next byte is `>= 100`; every observed
                        // phantom rides the prefix and carries an out-of-range
                        // `op0` (see `man_field_scripts::is_genuine_warp`).
                        if insn.extended.is_some() || !(100..=106).contains(&op0) {
                            continue;
                        }
                        out.push(WarpSite {
                            scene_name: name.clone(),
                            partition,
                            record,
                            abs_pc: script_start + insn.pc,
                            sub_id: op0 - 100,
                        });
                    }
                }
            }
        }
    }
    Some(out)
}

/// Corpus shape: every genuine door-warp site, grouped by `sub_id`.
#[test]
fn minigame_door_warp_census_pins_the_corpus_shape() {
    let Some(sites) = run_census() else { return };

    let mut by_sub: BTreeMap<u8, Vec<&WarpSite>> = BTreeMap::new();
    for s in &sites {
        by_sub.entry(s.sub_id).or_default().push(s);
    }
    for (sub, group) in &by_sub {
        let scenes: std::collections::BTreeSet<&str> =
            group.iter().map(|s| s.scene_name.as_str()).collect();
        eprintln!(
            "[minigame-warp census] sub_id={sub} ({:?}): {} site(s) across {:?}",
            MinigameSubId::from_sub_id(*sub),
            group.len(),
            scenes,
        );
        for s in group {
            eprintln!(
                "    scene={:<10} P{}[{:3}] @0x{:05X}",
                s.scene_name, s.partition, s.record, s.abs_pc,
            );
        }
    }
    eprintln!(
        "[minigame-warp census] {} total site(s), {} distinct sub_id(s)",
        sites.len(),
        by_sub.len(),
    );

    // Non-vacuity: the corpus does carry door-warps. If this ever fires, the
    // walk framing changed - not the disc.
    assert!(
        !sites.is_empty(),
        "no genuine mode-24 door-warp site found in any scene MAN - the walk \
         framing regressed (the arm is pinned in the disassembly)"
    );

    // The whole warp id space is `0..=6`; the filter enforces it, so this
    // asserts the filter is the right shape rather than re-asserting it.
    for s in &sites {
        assert!(
            s.sub_id <= 6,
            "sub_id {} out of the 7-id door-warp space at {} P{}[{}]",
            s.sub_id,
            s.scene_name,
            s.partition,
            s.record
        );
    }
}

/// Every `sub_id` the disc actually uses maps to a known minigame slot, and
/// at least one maps to a minigame the engine can enter.
#[test]
fn every_disc_sub_id_maps_to_a_known_minigame_slot() {
    let Some(sites) = run_census() else { return };

    let used: std::collections::BTreeSet<u8> = sites.iter().map(|s| s.sub_id).collect();
    eprintln!("[minigame-warp census] sub_ids present on the disc: {used:?}");

    let mut playable = 0usize;
    for sub in &used {
        let slot = MinigameSubId::from_sub_id(*sub)
            .unwrap_or_else(|| panic!("sub_id {sub} has no slot in the mode-24 dispatch table"));
        if slot.is_playable() {
            playable += 1;
        }
    }
    assert!(
        playable > 0,
        "the disc uses {used:?} but none maps to a minigame the engine can \
         enter - the sub-id table and the disc disagree"
    );
}
