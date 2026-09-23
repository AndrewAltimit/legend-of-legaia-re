//! Disc-gated: the **shipped shape** of the field-VM actor clone
//! (`0x4C` sub-1 sub-op `0x14`), taken off the scene MANs rather than off a
//! synthetic instruction, and driven through the port's own pool.
//!
//! The synthetic sibling (`field_actor_clone_op4c14.rs`) pins what one
//! instruction does. This one pins what the disc actually asks for, which is
//! the half a port can get right in isolation and wrong in aggregate:
//!
//!  - the instruction is **eight** bytes everywhere it ships, and the eighth
//!    byte is a cross-context actor id - a seven-byte read would leave the VM
//!    standing on that id, and `0x08` is not an opcode the field VM has an
//!    arm for, so the slide is not silent, it is a desync;
//!  - the whole disc issues exactly **three** distinct
//!    `(modulation word, rate)` pairs, so the after-image has three speeds
//!    and no more;
//!  - a burst's cadence and its clone's lifetime are set independently - the
//!    cadence by the `WaitFrames` between the instructions, the lifetime by
//!    the rate operand - and for every burst on the disc the two land so that
//!    **at most two copies are ever alive at once**. A port whose lifetime
//!    arithmetic is off by a factor shows up here as a third copy, which no
//!    per-instruction test can see.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.
//!
//! REF: FUN_801D835C (the clone helper), FUN_801D820C (the clone's tick)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_actor_clone::{CloneSource, clone_plan, modulation_rgb};
use legaia_engine_core::field_channels::FieldChannel;
use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::World;
use legaia_engine_vm::field::FieldCtx;
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

/// One decoded clone instruction, with the bytes it consumed.
#[derive(Debug, Clone)]
struct Site {
    scene: String,
    partition: usize,
    record: usize,
    /// Byte offset of the opcode inside the MAN payload.
    abs_pc: usize,
    size: usize,
    bytes: Vec<u8>,
    /// `WaitFrames` operand of the next instruction, when that instruction is
    /// one (`None` when the burst ends here).
    wait_after: Option<u16>,
}

impl Site {
    /// The `u24` the dispatcher's `FUN_8003CEB8` builds from operand bytes
    /// 1..3 - the clone's `+0x74` modulation colour.
    fn modulation(&self) -> u32 {
        u32::from(self.bytes[2])
            | (u32::from(self.bytes[3]) << 8)
            | (u32::from(self.bytes[4]) << 16)
    }
    /// The sign-extended `s16` from operand bytes 4..5 - the clone's `+0x54`
    /// per-vsync rate.
    fn rate(&self) -> i16 {
        i16::from_le_bytes([self.bytes[5], self.bytes[6]])
    }
    /// The eighth byte: the cross-context actor id the arm resolves.
    fn source_id(&self) -> u8 {
        self.bytes[7]
    }
}

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
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

/// Every clean `4C 14` site on the disc, scene-major.
///
/// "Clean" is the census rule: the linear walk had `CLEAN_RESYNC_INSNS`
/// good instructions of run-up, so the hit sits on a real instruction
/// boundary and not on an operand or a Shift-JIS byte that happens to read
/// `4C 14`.
fn census(root: &Path) -> Vec<Site> {
    let index = ProtIndex::open_extracted(root).expect("prot index");
    let mut out = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    let mut pending: Option<usize> = None;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            pending = None;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        // Close the previous site's cadence slot first: the
                        // `WaitFrames` that follows a clone is what spaces the
                        // burst.
                        if let Some(idx) = pending.take()
                            && let InsnInfo::WaitFrames { target } = insn.info
                        {
                            let site: &mut Site = &mut out[idx];
                            site.wait_after = Some(target);
                        }
                        if let InsnInfo::MenuCtrl { op0: 0x14, .. } = insn.info
                            && clean
                        {
                            let at = start + insn.pc;
                            out.push(Site {
                                scene: name.clone(),
                                partition,
                                record,
                                abs_pc: at,
                                size: insn.size,
                                bytes: man[at..at + insn.size].to_vec(),
                                wait_after: None,
                            });
                            pending = Some(out.len() - 1);
                        }
                    }
                }
            }
        }
    }
    out
}

#[test]
fn the_clone_ships_eight_bytes_wide_in_six_scenes_or_skip() {
    let Some(root) = extracted_root() else { return };
    let sites = census(&root);
    assert!(!sites.is_empty(), "the disc issues the clone op");

    let mut per_scene: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &sites {
        *per_scene.entry(s.scene.as_str()).or_default() += 1;
    }
    let expected: BTreeMap<&str, usize> = [
        ("kor5", 16),
        ("nilboa", 30),
        ("noaru", 12),
        ("retona", 13),
        ("urudre3", 7),
        ("vozz", 16),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        per_scene, expected,
        "six scenes issue the clone, and only these six"
    );
    assert_eq!(sites.len(), 94, "clone instructions shipped on the disc");

    // Eight bytes, every one of them. The eighth is the source id the arm
    // resolves; a seven-byte read would try to execute it.
    for s in &sites {
        assert_eq!(
            s.size, 8,
            "{} P{}[{}] @0x{:05X} is eight bytes wide",
            s.scene, s.partition, s.record, s.abs_pc
        );
        assert_eq!(s.bytes.len(), 8);
        assert_eq!(s.bytes[0], 0x4C);
        assert_eq!(s.bytes[1], 0x14);
    }
}

#[test]
fn the_whole_disc_uses_three_colour_rate_pairs_or_skip() {
    let Some(root) = extracted_root() else { return };
    let sites = census(&root);

    let pairs: BTreeSet<(u32, i16)> = sites.iter().map(|s| (s.modulation(), s.rate())).collect();
    let got: Vec<(u32, i16, [u8; 3], u32)> = pairs
        .iter()
        .map(|&(w, r)| {
            (
                w,
                r,
                modulation_rgb(w),
                clone_plan(CloneSource::default(), w, r).lifetime_vsyncs(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            // The `vozz` ramp: three steps of one word, the fastest fade.
            (0x001E_2832, 0x0199, [0x32, 0x28, 0x1E], 11),
            (0x0023_2D37, 0x0199, [0x37, 0x2D, 0x23], 11),
            (0x0028_323C, 0x0199, [0x3C, 0x32, 0x28], 11),
            // The common warm word, every other burst on the disc.
            (0x0028_3C3C, 0x00B2, [0x3C, 0x3C, 0x28], 24),
            // The blue-ish one, `noaru` only.
            (0x003F_2A2A, 0x0080, [0x2A, 0x2A, 0x3F], 32),
        ],
        "distinct (modulation, rate) pairs with their RGB + lifetime"
    );

    // Nothing on the disc sets the word's top byte, so the `sw` the helper
    // performs is a 24-bit write in practice.
    for s in &sites {
        assert_eq!(s.modulation() >> 24, 0);
        assert!(s.rate() > 0, "every shipped rate retires its clone");
    }

    // The source id is a cross-context actor id, never the executing context
    // and never a sentinel.
    let ids: BTreeSet<u8> = sites.iter().map(|s| s.source_id()).collect();
    assert_eq!(
        ids.into_iter().collect::<Vec<_>>(),
        vec![0x08, 0x09, 0x11, 0x13, 0x17, 0x1E, 0x20, 0x23, 0x25]
    );
}

/// Split a scene's sites into **bursts**.
///
/// A burst is a run of clone instructions separated only by `WaitFrames`:
/// the run ends at the first clone the script does not follow with one, which
/// is exactly the `wait_after: None` case. Nothing heuristic about it - the
/// cadence instruction is the join.
fn bursts<'a>(sites: &[&'a Site]) -> Vec<Vec<&'a Site>> {
    let mut out: Vec<Vec<&Site>> = Vec::new();
    let mut cur: Vec<&Site> = Vec::new();
    for s in sites {
        cur.push(s);
        if s.wait_after.is_none() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[test]
fn the_vozz_bursts_are_one_of_four_then_four_of_three_or_skip() {
    let Some(root) = extracted_root() else { return };
    let sites = census(&root);
    let vozz: Vec<&Site> = sites.iter().filter(|s| s.scene == "vozz").collect();

    // One record carries all sixteen: the cutscene timeline.
    for s in &vozz {
        assert_eq!((s.partition, s.record), (2, 13));
        assert_eq!(s.source_id(), 0x08, "one source actor for the whole scene");
        assert_eq!(s.rate(), 0x0199);
    }

    let shape: Vec<(usize, Vec<Option<u16>>)> = bursts(&vozz)
        .iter()
        .map(|b| (b.len(), b.iter().map(|s| s.wait_after).collect()))
        .collect();
    assert_eq!(
        shape,
        vec![
            (4, vec![Some(8), Some(8), Some(8), None]),
            (3, vec![Some(6), Some(6), None]),
            (3, vec![Some(8), Some(8), None]),
            (3, vec![Some(6), Some(6), None]),
            (3, vec![Some(6), Some(6), None]),
        ],
        "five bursts: one of four, four of three; two at eight frames, three at six"
    );

    // The colour walks the ramp within a burst, and restarts at its foot. The
    // four-clone burst issues the ramp's foot twice before walking, which is
    // why "three clones stepping the word" is a description of the ramp and
    // not of the first burst.
    let ramp = [0x001E_2832u32, 0x0023_2D37, 0x0028_323C];
    for (i, burst) in bursts(&vozz).iter().enumerate() {
        let expect: Vec<u32> = if burst.len() == 4 {
            vec![ramp[0], ramp[0], ramp[1], ramp[2]]
        } else {
            ramp.to_vec()
        };
        assert_eq!(
            burst.iter().map(|s| s.modulation()).collect::<Vec<_>>(),
            expect,
            "burst {i} colour ramp"
        );
    }
}

/// A world carrying one resolvable cross-context channel per script id the
/// burst names.
///
/// One channel is not enough: a single record can address more than one source
/// actor (`nilboa` P2 switches from `0x25` to `0x23` mid-record), and the arm's
/// `beqz s5` skips the helper for an id nothing resolves - so a one-channel
/// world silently measures a shorter burst.
fn world_with_sources(ids: &[u8]) -> World {
    let mut w = World::new();
    w.field_vm.channels = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| FieldChannel {
            placement_index: 3 + i,
            ctx: FieldCtx {
                script_id: u16::from(id),
                world_x: 0x0140,
                world_y: 0x0020,
                world_z: 0x0280,
                ..FieldCtx::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind: false,
        })
        .collect();
    w
}

#[test]
fn every_shipped_burst_stacks_the_depth_its_cadence_buys_or_skip() {
    let Some(root) = extracted_root() else { return };
    let sites = census(&root);

    let mut by_record: BTreeMap<(&str, usize, usize), Vec<&Site>> = BTreeMap::new();
    for s in &sites {
        by_record
            .entry((s.scene.as_str(), s.partition, s.record))
            .or_default()
            .push(s);
    }

    // Peak simultaneous clones per scene, measured by replaying each burst
    // against the port's own pool at the cadence the script asks for. The
    // depth is not a constant of the opcode: it is `ceil(lifetime / cadence)`,
    // and the disc spans 2..=6 because the cadence operand does.
    let mut peak_per_scene: BTreeMap<&str, usize> = BTreeMap::new();
    let mut seated_total = 0usize;
    let mut dropped_total = 0usize;
    for ((scene, partition, record), run) in &by_record {
        for burst in bursts(run) {
            let ids: Vec<u8> = burst
                .iter()
                .map(|s| s.source_id())
                .collect::<BTreeSet<u8>>()
                .into_iter()
                .collect();
            let mut w = world_with_sources(&ids);
            for s in &burst {
                let before: BTreeSet<usize> = w
                    .actors
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| a.active && a.handler == ActorHandler::ClipFade)
                    .map(|(i, _)| i)
                    .collect();
                w.field_bytecode = s.bytes.clone();
                w.field_pc = 0;
                w.step_field().expect("the clone instruction stepped");
                assert_eq!(w.field_pc, 8, "{scene} P{partition}[{record}] advanced 8");

                let live = w
                    .actors
                    .iter()
                    .filter(|a| a.active && a.handler == ActorHandler::ClipFade)
                    .count();
                let e = peak_per_scene.entry(scene).or_default();
                *e = (*e).max(live);

                // Colour and lifetime come off the instruction, not a table.
                // The freshly seated slot is the one the pool did not already
                // hold - the allocator reuses retired slots, so "the last
                // index" is not the new clone.
                match w
                    .actors
                    .iter()
                    .enumerate()
                    .find(|(i, a)| {
                        a.active && a.handler == ActorHandler::ClipFade && !before.contains(i)
                    })
                    .map(|(i, _)| i)
                {
                    Some(slot) => {
                        let seated = &w.actors[slot];
                        assert_eq!(seated.modulation_rgb, Some(modulation_rgb(s.modulation())));
                        assert_eq!(seated.physics.timer, s.rate());
                        seated_total += 1;
                    }
                    // `FUN_80020DE0` returns `0` on a full list and the field
                    // overlay's callers ignore it; with every source resolvable
                    // and a 64-slot pool, nothing here should take that path.
                    None => dropped_total += 1,
                }

                for _ in 0..s.wait_after.unwrap_or(0) {
                    w.tick_handler_actors(1);
                }
            }
        }
    }

    eprintln!(
        "[peak clones per scene] {peak_per_scene:?} seated={seated_total} dropped={dropped_total}"
    );
    assert_eq!(
        peak_per_scene,
        [
            ("kor5", 5),
            ("nilboa", 5),
            ("noaru", 5),
            ("retona", 6),
            ("urudre3", 5),
            ("vozz", 2),
        ]
        .into_iter()
        .collect::<BTreeMap<&str, usize>>(),
        "peak simultaneous clones per scene"
    );

    // The numbers are the script's, not the pool's: every one of the disc's
    // clone instructions found a slot, so no peak here is a capacity clamp.
    assert_eq!((seated_total, dropped_total), (94, 0));

    // Contrast, so the numbers are not a tautology of the pool: `vozz` is the
    // only scene whose cadence outruns its clone's life, and it is also the
    // only one with a rate that short.
    let vozz_life = clone_plan(CloneSource::default(), 0x001E_2832, 0x0199).lifetime_vsyncs();
    let common_life = clone_plan(CloneSource::default(), 0x0028_3C3C, 0x00B2).lifetime_vsyncs();
    assert!(vozz_life < common_life);
}
