//! Disc-gated census of the scene scripts' **camera roll** operand - and the
//! record of why a static sweep cannot settle the question it was aimed at.
//!
//! The port's cutscene camera composes pitch and yaw but drops roll, on the
//! stated assumption that "roll is rarely non-zero in retail shots". Nothing
//! had measured that against the disc's own camera operands, and "rarely" and
//! "never" are different claims with different consequences for a renderer.
//!
//! **What is measured.** Op `0x45` `CAMERA` sub-`0x00` (CONFIGURE) carries a
//! big-endian 10-bit slot mask `(op0 << 8) | op1`, bit `(9 - slot)`, then a
//! `u16` apply trigger and one signed 16-bit word per set slot in ascending
//! slot order. Slot `2` is the roll angle (`_DAT_8007B794`, the GTE
//! `RotMatrixZ` input); slot `7` is focus Y. See
//! `docs/subsystems/cutscene.md`. This walks every scene MAN's partition-1
//! actor-script records and tallies slot occupancy and roll operands.
//!
//! **What it cannot decide, and why.** A field-VM record's tail is not
//! linearly decodable - data follows code - so a sweep has to choose between
//! stopping at the first decode error (reaching ~21 CONFIGUREs corpus-wide,
//! far too few to conclude anything) and resuming a byte at a time (reaching
//! ~2000, but re-synchronising inside data). The resuming form is not merely
//! suspect, it is **provably** reading data: it reports roll operands outside
//! the 12-bit angle space `RotMatrixZ` masks its input to, and an authored
//! angle cannot be `26708`. Every intermediate error gate sits between those
//! two, and the non-zero count moves monotonically with the threshold with no
//! plateau - so a gated census measures the gate, not the disc.
//!
//! Deciding it needs **execution**, not linear decode: run each candidate
//! record through the ported field VM and read the roll its CONFIGURE actually
//! commits. The candidates are the scenes the resuming sweep flags with
//! in-range, repeating operands - `deroa`, `chitei2`, `station3`, `town0b`,
//! `retona`, `nilboa`, `edstati3`.
//!
//! The assertions below are what survives regardless of that choice.
//! Skips silently when `extracted/PROT/` is missing.

use legaia_asset::field_disasm::{CameraKind, InsnInfo, decode};
use legaia_asset::{man_section, scene_asset_table};
use std::path::PathBuf;

/// Slot index of the roll angle in the op-`0x45` CONFIGURE mask.
const ROLL_SLOT: usize = 2;
/// Slot index of the focus-Y param - the corpus's independently-known rare slot.
const FOCUS_Y_SLOT: usize = 7;
/// Slots a CONFIGURE mask can carry.
const SLOTS: usize = 10;
/// `RotMatrixX/Y/Z` mask their angle argument to 12 bits (`4096` = 360 deg),
/// so an operand at or beyond this cannot be an authored camera angle.
const ANGLE_SPACE: i32 = 4096;

fn extracted_prot() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// Every partition-1 actor script body in one parsed MAN, as `(start, end)`.
///
/// Each record shares the prefix `[u8 local_count N][N * 2][4-byte header]`
/// (`man_section::ManFile::scene_entry_script` documents it); the field-VM body
/// begins after it and runs to the next record's offset.
fn actor_script_bodies(man: &man_section::ManFile, bytes: &[u8]) -> Vec<(usize, usize)> {
    let n1 = man.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man.actor_placement_record_offset(i, bytes.len()))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let mut out = Vec::new();
    for (k, &off) in starts.iter().enumerate() {
        let Some(&locals) = bytes.get(off) else {
            continue;
        };
        let body = off + 1 + locals as usize * 2 + 4;
        let end = starts.get(k + 1).copied().unwrap_or(bytes.len());
        if body < end && end <= bytes.len() {
            out.push((body, end));
        }
    }
    out
}

#[derive(Default)]
struct Census {
    scenes: usize,
    configures: usize,
    /// How many CONFIGUREs set each slot.
    slot_set: [usize; SLOTS],
    /// Roll operands written, each with the scene that wrote it.
    rolls: Vec<(i16, String)>,
}

/// Walk one record's bytecode. `resume` picks the two sweep modes the module
/// docs contrast: `false` stops at the first decode error (under-counts, never
/// invents), `true` advances a byte and continues (reaches far more, and
/// re-synchronises inside data).
fn walk(body: &[u8], scene: &str, resume: bool, c: &mut Census) {
    let mut pc = 0usize;
    while pc < body.len() {
        let insn = match decode(body, pc) {
            Ok(i) => i,
            Err(_) if resume => {
                pc += 1;
                continue;
            }
            Err(_) => return,
        };
        if let InsnInfo::Camera {
            kind: CameraKind::Configure { mask, .. },
            ..
        } = insn.info
        {
            let set: Vec<usize> = (0..SLOTS).filter(|s| mask & (1 << (9 - s)) != 0).collect();
            // `size = header_size + 4 + 2 * set_count`, and the operand words
            // follow the two mask bytes and the u16 apply trigger.
            let vals = pc + insn.size.saturating_sub(4 + 2 * set.len()) + 4;
            c.configures += 1;
            for (k, &slot) in set.iter().enumerate() {
                c.slot_set[slot] += 1;
                let at = vals + 2 * k;
                if slot == ROLL_SLOT && at + 2 <= body.len() {
                    c.rolls
                        .push((i16::from_le_bytes([body[at], body[at + 1]]), scene.into()));
                }
            }
        }
        pc += insn.size.max(1);
    }
}

fn census(resume: bool) -> Option<Census> {
    let prot = extracted_prot()?;
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&prot)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("BIN"))
        .collect();
    paths.sort();

    let mut c = Census::default();
    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Some(table) = scene_asset_table::detect(&bytes) else {
            continue;
        };
        let Some(d) = table.descriptors.iter().find(|d| d.type_byte == 0x03) else {
            continue;
        };
        let start = d.data_offset as usize;
        if start >= bytes.len() {
            continue;
        }
        let Ok((man, _)) = legaia_lzs::decompress_tracked(&bytes[start..], d.size as usize) else {
            continue;
        };
        if man.len() as u32 != d.size {
            continue;
        }
        let Ok(parsed) = man_section::parse(&man) else {
            continue;
        };
        c.scenes += 1;
        let scene = path.file_name().unwrap().to_string_lossy().to_string();
        for (lo, hi) in actor_script_bodies(&parsed, &man) {
            walk(&man[lo..hi], &scene, resume, &mut c);
        }
    }
    (c.scenes > 0).then_some(c)
}

/// Roll is an *optional* slot: most camera beats never write it, while pitch,
/// yaw, the eye trio, focus X/Z and H are set by nearly all of them. The ratio
/// holds in every sweep mode, so it is a property of the disc and not of the
/// walk - which is what separates it from the non-zero count.
#[test]
fn roll_is_an_optional_slot_most_camera_beats_never_set() {
    let Some(c) = census(true) else {
        eprintln!("[skip] extracted/PROT/ missing or no scene MANs recovered");
        return;
    };
    eprintln!(
        "[camera-roll census] {} scenes, {} CONFIGURE ops, slot occupancy {:?}",
        c.scenes, c.configures, c.slot_set
    );
    assert!(
        c.configures > 100,
        "expected the camera op across the scene corpus, got {}",
        c.configures
    );

    // The eight routinely-set slots, as a floor to compare the optional ones to.
    let common: usize = (0..SLOTS)
        .filter(|s| *s != ROLL_SLOT && *s != FOCUS_Y_SLOT)
        .map(|s| c.slot_set[s])
        .min()
        .unwrap();
    assert!(
        c.slot_set[ROLL_SLOT] * 2 < common,
        "roll should be set by a minority of beats: roll {} vs the common-slot floor {common}",
        c.slot_set[ROLL_SLOT],
    );
    // Cross-check the mask decode against a fact established independently of
    // this sweep: focus Y is the rarest slot of all (`cutscene.md` - opdeene's
    // beats supply focus X and Z but never Y). A wrong bit order breaks this.
    assert!(
        c.slot_set[FOCUS_Y_SLOT] * 10 < c.slot_set[ROLL_SLOT],
        "focus Y should be far rarer than roll: focus-Y {} vs roll {}",
        c.slot_set[FOCUS_Y_SLOT],
        c.slot_set[ROLL_SLOT]
    );
}

/// The resuming sweep re-synchronises inside data, so its non-zero rolls are
/// not evidence - and this *proves* that rather than asserting it, by finding
/// roll operands outside the 12-bit space the GTE masks its angle argument to.
///
/// Kept as a test so a future reachability walker has to retire the row: if a
/// control-flow walk ever reaches these CONFIGUREs cleanly and no out-of-range
/// operand survives, the question becomes statically decidable and the module
/// doc above is what needs rewriting.
#[test]
fn a_resuming_linear_sweep_reads_data_as_camera_operands() {
    let Some(resumed) = census(true) else {
        eprintln!("[skip] extracted/PROT/ missing or no scene MANs recovered");
        return;
    };
    let Some(strict) = census(false) else {
        return;
    };

    let impossible: Vec<i16> = resumed
        .rolls
        .iter()
        .map(|(v, _)| *v)
        .filter(|v| i32::from(*v).abs() >= ANGLE_SPACE)
        .collect();
    eprintln!(
        "[roll sweep modes] resuming: {} CONFIGUREs, {} roll operands, {} outside the angle \
         space; strict: {} CONFIGUREs, {} roll operands",
        resumed.configures,
        resumed.rolls.len(),
        impossible.len(),
        strict.configures,
        strict.rolls.len()
    );
    assert!(
        !impossible.is_empty(),
        "the resuming sweep was expected to decode data as camera operands; if it no longer \
         does, a static census may now be able to settle the roll question"
    );

    // The strict sweep invents nothing, and everything it reaches writes zero.
    let non_zero: Vec<&(i16, String)> = strict.rolls.iter().filter(|(v, _)| *v != 0).collect();
    assert!(
        non_zero.is_empty(),
        "a CONFIGURE reached without any error resume writes a non-zero roll: {non_zero:?}"
    );
}
