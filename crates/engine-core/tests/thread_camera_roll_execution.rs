//! Disc-gated **executing** census of the field-VM camera-roll operand, and
//! the answer to "does any retail shot author a non-zero camera roll?".
//!
//! **It does.** Eight scenes stage a reachable, executing op-`0x45` CONFIGURE
//! that writes slot `2` (`_DAT_8007B794`, the GTE `RotMatrixZ` angle) to a
//! non-zero in-range value, from a 0.9-degree lean to a 58-degree Dutch angle.
//! The per-scene table is on
//! [`retail_camera_beats_author_a_non_zero_roll`].
//!
//! ## Why the linear censuses could not settle it
//!
//! A field-VM record's tail is not linearly decodable - data follows code - so
//! a *linear* sweep has to choose between stopping at the first decode error
//! (which reaches almost nothing) and resuming a byte at a time (which
//! re-synchronises inside data and reports operands no authored camera angle
//! can hold). Every error gate between the two moves the non-zero count
//! monotonically, so a gated linear census measures its own gate. A raw byte
//! scan - trying to decode a CONFIGURE at *every* byte offset - is the same
//! failure with no gate at all, and its "N% of beats roll" ratio is a property
//! of whatever credibility filter it applies afterwards.
//!
//! ## What decides it
//!
//! Control flow. Two instruments, both driven by the ported field VM
//! ([`legaia_engine_vm::field::step`]) rather than by a re-derivation of its
//! PC arithmetic:
//!
//! 1. **Reachability.** [`reachable_configures`] walks each record from its
//!    real entry PC, stepping the VM under a probe host that answers every
//!    predicate both ways, and unions the resulting next-PCs. A PC in the
//!    result is one control flow can arrive at; a PC outside it is bytes the
//!    VM never decodes. This is a superset of every execution path.
//! 2. **Execution.** [`exec_census`] loads the same records into a real
//!    [`World`] and steps them, so a CONFIGURE counted there has run through
//!    the whole engine chain - VM step, `camera_configure` host hook,
//!    `FieldEvent::CameraConfigure`, `World::camera_state` merge. It runs
//!    twice, once with the flag banks cleared and once full, so both arms of
//!    every story-flag gate execute.
//!
//! Neither reaches a roll operand outside the 12-bit angle space, which is
//! what identifies the linear sweeps' out-of-range operands as data.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_asset::man_section;
use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field::{
    ActorSearchResult, CameraParam, FieldCtx, FieldHost, StepResult, step,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Slot index of the roll angle in the op-`0x45` CONFIGURE mask.
const ROLL_SLOT: u8 = 2;
/// `RotMatrixX/Y/Z` mask their angle argument to 12 bits (`4096` = 360 deg),
/// so an operand at or beyond this cannot be an authored camera angle.
const ANGLE_SPACE: i32 = 4096;
/// Upper bound on VM steps spent on one record in the execution pass. Records
/// park on waits and polls; the walk is bounded rather than run to quiescence.
const EXEC_STEP_BUDGET: usize = 4_000;
/// Upper bound on distinct PCs visited per record in the reachability pass.
const REACH_PC_BUDGET: usize = 20_000;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// One CONFIGURE the census reached, keyed by where it lives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
    scene: String,
    entry_idx: u32,
    partition: usize,
    record: usize,
    /// PC of the opcode inside the record body.
    pc: usize,
    /// The roll operand, when the beat's mask set slot 2.
    roll: Option<i16>,
    /// Every `(slot, value)` the beat's mask carried, so a reported roll can
    /// be read next to the pitch / yaw / eye / focus / H it was staged with.
    params: Vec<(u8, i16)>,
}

#[derive(Default)]
struct Census {
    scenes: usize,
    records: usize,
    /// Records the walk entered at all (non-empty body, decodable entry).
    records_walked: usize,
    configures: usize,
    /// Every CONFIGURE that set slot 2, whatever the value.
    roll_sites: Vec<Site>,
}

impl Census {
    fn non_zero(&self) -> Vec<&Site> {
        self.roll_sites
            .iter()
            .filter(|s| s.roll.is_some_and(|v| v != 0))
            .collect()
    }

    fn out_of_range(&self) -> Vec<&Site> {
        self.roll_sites
            .iter()
            .filter(|s| s.roll.is_some_and(|v| i32::from(v).abs() >= ANGLE_SPACE))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Reachability: step the real VM under a probe host that answers both ways.
// ---------------------------------------------------------------------------

/// A [`FieldHost`] whose every branch predicate answers one way, so stepping
/// the same PC under both polarities yields both arms of every conditional the
/// VM routes through the host. It has no world: the only state it keeps is the
/// CONFIGURE payloads the VM hands it, which is how a reached beat's operands
/// are read without re-deriving the operand offsets here.
#[derive(Default)]
struct ProbeHost {
    yes: bool,
    configures: Vec<Vec<CameraParam>>,
}

impl FieldHost for ProbeHost {
    fn global_flags(&self) -> u32 {
        if self.yes { u32::MAX } else { 0 }
    }
    fn set_global_flags(&mut self, _value: u32) {}
    fn frame_delta(&self) -> u16 {
        // The wait accumulator is an `i16`, so this drains every authored
        // `WAIT_FRAMES` target below `0x8000` in a single step.
        i16::MAX as u16
    }
    fn extra_flags(&self) -> u32 {
        if self.yes { u32::MAX } else { 0 }
    }
    fn screen_mode(&self) -> u32 {
        if self.yes { u32::MAX } else { 0 }
    }
    fn screen_mode_table(&self, _index: u8) -> Option<u32> {
        self.yes.then_some(0xF000)
    }
    fn system_flag_test(&self, _idx: u16) -> bool {
        self.yes
    }
    fn field_halt_acquire_predicate(&self, _ctx: &FieldCtx, _which: u8) -> bool {
        self.yes
    }
    fn op4c_n8_sub_c_branch_on_field_68(&self, _ctx: &FieldCtx) -> bool {
        self.yes
    }
    fn op4c_n_8_sub_b_actor_type_present(&self, _type_byte: u8) -> bool {
        self.yes
    }
    fn op4c_n_8_sub_d_actor_search(&self, _char_idx: u8, _marker: u8) -> ActorSearchResult {
        if self.yes {
            ActorSearchResult::Found
        } else {
            ActorSearchResult::EmptySlot
        }
    }
    fn op4c_n_e_sub_b_actor_jump(&mut self, _actor_id: u8) -> Option<()> {
        self.yes.then_some(())
    }
    fn op4c_n_e_sub_4_bbox_outside(&self, _ctx: &FieldCtx, _bbox: [i16; 4]) -> bool {
        self.yes
    }
    fn op4c_n_c_party_flag_test(&self, _flag_idx: u16) -> bool {
        self.yes
    }
    fn op4c_n_a_flag_set(&self, _ctx: &FieldCtx, _bank: u8, _bit: u8) -> bool {
        self.yes
    }
    fn op4c_nibble4_global_pair_gate(&self) -> bool {
        self.yes
    }
    fn op4c_n_c_sub9_globals_differ(&self) -> bool {
        self.yes
    }
    fn inventory_compare_pair(&self, _page: u8, _sub_op: u8) -> (i32, i32) {
        if self.yes {
            (i32::MAX, 0)
        } else {
            (0, i32::MAX)
        }
    }
    fn camera_configure(&mut self, params: &[CameraParam], _apply_trigger: u16, _mode: u8) {
        self.configures.push(params.to_vec());
    }
}

/// The PCs of `body` control flow can reach from `entry_pc`, plus the
/// CONFIGURE payload the VM decodes at each such PC that carries one.
///
/// Successors come from the VM itself: `step` is run at each PC under a set of
/// probe configurations that between them take both arms of every conditional
/// the VM routes through the host trait or through `ctx`, and the union of the
/// resulting next-PCs is the successor set. Nothing here re-derives a PC
/// delta, so the walk cannot drift from the executing port.
///
/// A PC whose step reports `Unknown` (the dispatcher's "UNFIND INDICATION"
/// arm - a byte outside the opcode space) is a dead end: retail's run-until-
/// yield loop breaks on exactly that condition, so the path stops rather than
/// resynchronising into the following bytes. `Pending` (an op-`0x4C` nibble-8
/// sub-op the port does not size) is treated the same way, which makes the
/// walk an **under**-approximation there - the safe direction for a census
/// that is looking for something, since a missed successor can only lose
/// sites, never invent one.
fn reachable_configures(
    body: &[u8],
    entry_pc: usize,
) -> (BTreeSet<usize>, Vec<(usize, Vec<CameraParam>)>) {
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    let mut work = vec![entry_pc];
    let mut configures: Vec<(usize, Vec<CameraParam>)> = Vec::new();
    while let Some(pc) = work.pop() {
        if pc >= body.len() || !seen.insert(pc) || seen.len() > REACH_PC_BUDGET {
            continue;
        }
        // Probe seeds. `yes` flips every host predicate; the ctx variants flip
        // the two conditionals the VM reads off `ctx` rather than the host
        // (0x33 CFLAG_TST on `ctx.flags`, 0x4D BBOX_TEST on `ctx.world_*`).
        // The bbox seed puts the context deep inside the positive tile
        // quadrant; the zero seed leaves it at tile `-1`, outside every box
        // whose minimum is non-negative, so the two seeds cover both arms.
        let seeds: [(bool, u32, u16); 4] = [
            (false, 0, 0),
            (true, u32::MAX, 0),
            (false, 0, 0x4000),
            (true, u32::MAX, 0x4000),
        ];
        for (yes, ctx_flags, world) in seeds {
            let mut host = ProbeHost {
                yes,
                configures: Vec::new(),
            };
            let mut ctx = FieldCtx {
                flags: ctx_flags,
                local_flags: if yes { u16::MAX } else { 0 },
                world_x: world,
                world_z: world,
                ..FieldCtx::default()
            };
            let res = step(&mut host, &mut ctx, body, pc);
            if let Some(params) = host.configures.pop()
                && !configures.iter().any(|(p, _)| *p == pc)
            {
                configures.push((pc, params));
            }
            let next = match res {
                StepResult::Advance { next_pc } => Some(next_pc),
                StepResult::Yield { resume_pc } => Some(resume_pc),
                // A halt at the same PC is a stall (a wait or a failed flag
                // gate), not a successor; a halt elsewhere is a real move.
                StepResult::Halt { final_pc } if final_pc != pc => Some(final_pc),
                _ => None,
            };
            if let Some(n) = next
                && n != pc
                && n < body.len()
            {
                work.push(n);
            }
        }
    }
    (seen, configures)
}

// ---------------------------------------------------------------------------
// Corpus walk
// ---------------------------------------------------------------------------

/// One walkable record: `(scene, entry_idx, partition, record, body, entry_pc)`.
type Record = (String, u32, usize, usize, Vec<u8>, usize);

/// Every record in the disc's
/// MAN corpus - every CDNAME scene, every MAN carrier (bundle plus the
/// standalone story-state variants), all three partitions.
fn corpus(index: &ProtIndex, scenes: &[String]) -> Vec<Record> {
    let mut out = Vec::new();
    for name in scenes {
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
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
                    out.push((
                        name.clone(),
                        carrier.entry_idx,
                        partition,
                        record,
                        body.to_vec(),
                        pc0,
                    ));
                }
            }
        }
    }
    out
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

/// Reachability census over the whole corpus.
fn reach_census(index: &ProtIndex, corpus: &[Record]) -> Census {
    let _ = index;
    let mut c = Census::default();
    let mut scenes = BTreeSet::new();
    for (scene, entry_idx, partition, record, body, pc0) in corpus {
        scenes.insert(scene.clone());
        c.records += 1;
        if body.is_empty() || *pc0 >= body.len() {
            continue;
        }
        c.records_walked += 1;
        let (_pcs, configures) = reachable_configures(body, *pc0);
        for (pc, params) in configures {
            c.configures += 1;
            if let Some(p) = params.iter().find(|p| p.slot == ROLL_SLOT) {
                c.roll_sites.push(Site {
                    scene: scene.clone(),
                    entry_idx: *entry_idx,
                    partition: *partition,
                    record: *record,
                    pc,
                    roll: Some(p.value as i16),
                    params: params.iter().map(|p| (p.slot, p.value as i16)).collect(),
                });
            }
        }
    }
    c.scenes = scenes.len();
    c
}

/// Execution census: load each record into a real `World` and step it.
///
/// `flags_set` seeds the story / system / extra flag banks full, so the pass
/// takes the *other* arm of every flag-gated branch than the cleared pass -
/// two executions per record between them cover both sides of the story gates
/// without leaving the executing engine.
fn exec_census(corpus: &[Record], flags_set: bool) -> Census {
    let mut c = Census::default();
    let mut scenes = BTreeSet::new();
    for (scene, entry_idx, partition, record, body, pc0) in corpus {
        scenes.insert(scene.clone());
        c.records += 1;
        if body.is_empty() || *pc0 >= body.len() {
            continue;
        }
        c.records_walked += 1;
        let mut world = World {
            mode: SceneMode::Field,
            ..World::default()
        };
        if flags_set {
            world.story_flags = u32::MAX;
            world.extra_flags = u32::MAX;
            world.system_flags = vec![0xFF; world.system_flags.len().max(0x200)];
        }
        world.load_field_script_at(body.clone(), *pc0);
        let mut last_pc = usize::MAX;
        let mut stall = 0usize;
        for _ in 0..EXEC_STEP_BUDGET {
            let pc = world.field_pc;
            let Some(res) = world.step_field() else { break };
            for ev in world.drain_field_events() {
                let legaia_engine_core::field_events::FieldEvent::CameraConfigure {
                    params, ..
                } = ev
                else {
                    continue;
                };
                c.configures += 1;
                if let Some(p) = params.iter().find(|p| p.slot == ROLL_SLOT) {
                    c.roll_sites.push(Site {
                        scene: scene.clone(),
                        entry_idx: *entry_idx,
                        partition: *partition,
                        record: *record,
                        pc,
                        roll: Some(p.value as i16),
                        params: params.iter().map(|p| (p.slot, p.value as i16)).collect(),
                    });
                }
            }
            match res {
                StepResult::Unknown { .. } => break,
                StepResult::Pending { .. } => break,
                _ => {}
            }
            if world.field_pc == last_pc {
                stall += 1;
                if stall > 64 {
                    break;
                }
            } else {
                stall = 0;
                last_pc = world.field_pc;
            }
        }
    }
    c.scenes = scenes.len();
    c
}

fn report(label: &str, c: &Census) {
    let nz = c.non_zero();
    let oor = c.out_of_range();
    eprintln!(
        "[{label}] {} scenes, {} records ({} walked), {} CONFIGUREs, {} set roll, \
         {} non-zero, {} outside the 12-bit angle space",
        c.scenes,
        c.records,
        c.records_walked,
        c.configures,
        c.roll_sites.len(),
        nz.len(),
        oor.len()
    );
    let mut by_value: BTreeMap<i16, usize> = BTreeMap::new();
    for s in &nz {
        *by_value.entry(s.roll.unwrap()).or_default() += 1;
    }
    if !by_value.is_empty() {
        eprintln!("[{label}] non-zero roll operands (value -> count): {by_value:?}");
        for s in nz.iter().take(24) {
            eprintln!(
                "[{label}]   {} entry {} P{}[{}] pc 0x{:04X} roll {:>5} ({:>6.2} deg) slots {:?}",
                s.scene,
                s.entry_idx,
                s.partition,
                s.record,
                s.pc,
                s.roll.unwrap(),
                f64::from(s.roll.unwrap()) * 360.0 / 4096.0,
                s.params,
            );
        }
    }
}

/// The census, and the answer: **retail authors a non-zero camera roll.**
///
/// Reachability is the superset - a CONFIGURE outside the reachable PC set is
/// bytes the VM never decodes - and execution is the subset that actually
/// runs. Both find the same population, and it is coherent rather than
/// accidental: every roll-setting beat below carries the full nine-slot mask
/// (pitch, yaw, roll, the eye trio, focus X/Z, H - no focus Y, matching the
/// `opdeene` reading on `cutscene.md`), every operand is an in-range 12-bit
/// angle, and the beats of one shot repeat the same tilt.
///
/// Eight scenes stage one, from a 0.9-degree lean to a 58-degree Dutch angle:
///
/// | scene | PROT entry | roll (12-bit) | degrees |
/// |---|---|---|---|
/// | `edstati3` | 826 | `10`, `20` | 0.9, 1.8 |
/// | `station3` | 616 | `30` | 2.6 |
/// | `map03` | 392 | `60` | 5.3 |
/// | `nilboa` | 638 | `60` | 5.3 |
/// | `taiku` | 373 | `-120` | -10.5 |
/// | `korout` | 534 | `240` | 21.1 |
/// | `juui1` | 588 | `-400` | -35.2 |
/// | `juui2` | 597 | `-660` | -58.0 |
///
/// The assertions pin the shape rather than the exact tally, so a decoder
/// improvement that reaches more records does not have to be re-baselined -
/// but the *floor* is asserted, because losing these sites again would mean
/// the roll wiring had gone back to being untestable.
#[test]
fn retail_camera_beats_author_a_non_zero_roll() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let Ok(index) = ProtIndex::open_extracted(&extracted) else {
        eprintln!("[skip] PROT index would not open");
        return;
    };
    let names = scene_names(&extracted);
    assert!(!names.is_empty(), "CDNAME yielded no scene names");
    let corpus = corpus(&index, &names);
    assert!(
        corpus.len() > 1_000,
        "only {} MAN records recovered - the corpus walk is not reaching the disc",
        corpus.len()
    );

    let reach = reach_census(&index, &corpus);
    report("reach", &reach);
    let exec_clear = exec_census(&corpus, false);
    report("exec flags-clear", &exec_clear);
    let exec_set = exec_census(&corpus, true);
    report("exec flags-set", &exec_set);

    // Non-vacuity: the walks have to be finding the camera op at all.
    assert!(
        reach.configures > 100,
        "the reachability walk found only {} CONFIGUREs - it is not reaching the corpus",
        reach.configures
    );
    assert!(
        exec_clear.configures > 100,
        "execution found only {} CONFIGUREs on the flags-clear pass",
        exec_clear.configures
    );

    // The answer. A control-flow walk reaches beats that author a non-zero
    // roll, and EXECUTION commits them - so a camera composing pitch and yaw
    // only frames those shots wrong.
    let scenes_with_roll =
        |c: &Census| -> BTreeSet<String> { c.non_zero().iter().map(|s| s.scene.clone()).collect() };
    let reach_scenes = scenes_with_roll(&reach);
    let exec_scenes = scenes_with_roll(&exec_clear);
    for want in [
        "edstati3", "juui1", "juui2", "korout", "map03", "nilboa", "station3", "taiku",
    ] {
        assert!(
            reach_scenes.contains(want),
            "scene `{want}` stages a reachable non-zero roll; reachability found {reach_scenes:?}"
        );
        assert!(
            exec_scenes.contains(want),
            "scene `{want}`'s roll beat EXECUTES; execution found {exec_scenes:?}"
        );
    }
    // The extremes, so the magnitude claim in the docs stays pinned to data.
    let values: BTreeSet<i16> = reach.non_zero().iter().filter_map(|s| s.roll).collect();
    assert!(values.contains(&-660), "juui2's -58 deg tilt: {values:?}");
    assert!(values.contains(&10), "edstati3's 0.9 deg lean: {values:?}");

    // Every reachable roll operand is an in-range angle. This is what
    // separates this census from the linear ones: the resuming linear sweep
    // reports operands outside the 12-bit space `RotMatrixZ` masks its
    // argument to (`26708` is not an authored angle) and a raw byte scan
    // reports more still, because both decode data. A control-flow walk
    // reaches none of them.
    assert!(
        reach.out_of_range().is_empty(),
        "a reachable roll operand is outside the 12-bit angle space: {:?}",
        reach.out_of_range()
    );

    // Roll stays a MINORITY term - roughly a third of reached beats set the
    // slot at all, and about one in eight of those writes a non-zero value.
    // That is why "roll is rarely non-zero" survived as an assumption for so
    // long. Rare is not never, and the renderer has to carry the term.
    assert!(
        reach.non_zero().len() * 4 < reach.roll_sites.len(),
        "non-zero rolls should stay a minority of roll-setting beats: {} of {}",
        reach.non_zero().len(),
        reach.roll_sites.len()
    );
}
