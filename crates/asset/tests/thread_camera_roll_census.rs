//! Disc-gated census of the scene scripts' **camera roll** operand under two
//! *linear* sweep modes - kept as the record of why neither can answer the
//! question, now that the question is answered elsewhere.
//!
//! **The answer.** Retail authors a non-zero camera roll: eight scenes stage
//! a control-flow-reachable, executing op-`0x45` CONFIGURE that writes slot
//! `2` (`_DAT_8007B794`, the GTE `RotMatrixZ` angle) to a non-zero in-range
//! value, from a 0.9-degree lean to a 58-degree Dutch angle. That was settled
//! by execution, in `crates/engine-core/tests/thread_camera_roll_execution.rs`,
//! which steps every MAN record through the ported field VM and reads the roll
//! each reached CONFIGURE commits. This file measures the two linear modes so
//! the reason a *decode* cannot substitute stays testable.
//!
//! **What is measured here.** Op `0x45` `CAMERA` sub-`0x00` (CONFIGURE)
//! carries a big-endian 10-bit slot mask `(op0 << 8) | op1`, bit `(9 - slot)`,
//! then a `u16` apply trigger and one signed 16-bit word per set slot in
//! ascending slot order. Slot `2` is the roll angle; slot `7` is focus Y. See
//! `docs/subsystems/cutscene.md`. This walks every scene MAN's partition-1
//! actor-script records and tallies slot occupancy and roll operands.
//!
//! **Why a linear sweep cannot decide it.** A field-VM record's tail is not
//! linearly decodable - data follows code - so a sweep has to choose between
//! stopping at the first decode error and resuming a byte at a time, and the
//! two disagree by two orders of magnitude. Both are wrong in opposite
//! directions, which the executing census makes concrete:
//!
//! - the **resuming** mode over-reads. It re-synchronises inside data and
//!   reports roll operands outside the 12-bit space `RotMatrixZ` masks its
//!   argument to - an authored angle cannot be `26708`. The control-flow walk
//!   reaches none of those, so they are data.
//! - the **strict** mode under-reads, and dangerously: it reaches a couple of
//!   dozen CONFIGUREs corpus-wide and **not one** of the eight rolled shots.
//!   Its silence on non-zero roll was blindness, not evidence.
//!
//! Every error gate between the two moves the count monotonically with no
//! plateau, so a gated linear census measures its own threshold. A raw byte
//! scan (decode at every offset) is the same failure with no gate at all.
//!
//! **And a second blindness, structural rather than statistical:** this walk
//! covers **partition 1** (actor placements), while every authored roll on
//! the disc lives in **partition 2** - the cutscene-timeline / walk-on beat
//! records. So the strict mode was not merely under-reading its own corpus,
//! it was reading a different one. Scoping a census to the partition that is
//! easy to enumerate, rather than to the partition the feature lives in, is
//! the shape to watch for.
//!
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
/// The executing census reaches no such operand, which is what turns "these
/// look wrong" into "these are data".
///
/// The strict sweep is the opposite failure and the more dangerous one: it
/// invents nothing, so everything it reaches is real - but it reaches so
/// little that it misses **every** authored roll on the disc. Both halves are
/// asserted here so neither mode can be quoted as an answer again.
#[test]
fn neither_linear_sweep_mode_can_answer_the_roll_question() {
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
        "the resuming sweep decodes data as camera operands - an authored roll cannot lie \
         outside the 12-bit angle space, and the control-flow walk reaches none of these"
    );

    // The strict sweep's blindness, stated as the two facts that make it one:
    // it reaches a tiny fraction of the corpus's CONFIGUREs, and none of what
    // it does reach is one of the authored rolls.
    assert!(
        strict.configures * 10 < resumed.configures,
        "the strict sweep should reach an order of magnitude fewer CONFIGUREs than the \
         resuming one ({} vs {})",
        strict.configures,
        resumed.configures
    );
    let strict_non_zero: Vec<&(i16, String)> =
        strict.rolls.iter().filter(|(v, _)| *v != 0).collect();
    assert!(
        strict_non_zero.is_empty(),
        "the strict sweep is blind to every authored roll on the disc - it must not be the \
         thing that reports one: {strict_non_zero:?}"
    );
}
