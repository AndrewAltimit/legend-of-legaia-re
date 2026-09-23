//! Disc-gated oracle for the field SFX cue ring's producers: which scenes
//! push scripted cue ids, and that every id a scene pushes resolves to a
//! descriptor row the hosts can key.
//!
//! Retail's producers are the field VM's op `0x36` sub-`0` (`FUN_80035B50`
//! from `0x801E0348`, the id is the op's second `s16`) with its delay partner
//! sub-`4` (`FUN_80035BAC` from `0x801E03D8`), and the ambient motion VM's op
//! `0x09` (`FUN_80035B50` from `0x80039178`). The engine hands each call to
//! the hosts as a `SfxRingOp` (`World::take_sfx_ring_ops`); this test drives
//! real scenes through `SceneHost` and reads that surface, the one both hosts
//! drain.
//!
//! What it pins:
//!
//! - Authored records reach the ring with their own ids: `opdeene`'s
//!   partition-2 record 6 pushes `0x2A` and `0x29`, each followed by the
//!   delay write the script pairs with it; `town01`'s partition-0 record 1
//!   pushes `0x2E`.
//! - No scene reaches either producer **unprompted** in its first 4000 ticks:
//!   the census over every CDNAME field scene records which do (none, on the
//!   retail disc) - the sites sit behind interactions, walk-ons and timeline
//!   branches a no-input run does not take.
//! - Every pushed id resolves: below `0x200` into the static table's 100
//!   rows, at or above it into a populated row of the scene's own prescript
//!   record 0 (`World::runtime_sfx_descriptor`).
//! - opdeene's prescript record 16 raises the frame-step floor to `3` (its
//!   first op is move-VM `0x2F` sub-`0x2F` with operand `3`), which is the
//!   value the cold-boot capture reads at `DAT_8007B9D8`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` or `extracted/` is missing.

use legaia_asset::field_disasm as census;
use legaia_asset::man_section;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::SfxRingOp;
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

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// Drive `scene` for `ticks` frames with no input and collect every ring op.
fn ring_ops(extracted: &PathBuf, scene: &str, ticks: u32) -> (SceneHost, Vec<(u32, SfxRingOp)>) {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene(scene, 0).expect("enter scene");
    let mut ops = Vec::new();
    for t in 0..ticks {
        host.world.set_pad(0);
        let _ = host.world.tick();
        ops.extend(host.world.take_sfx_ring_ops().into_iter().map(|o| (t, o)));
        // Stop at a scripted scene change: the next scene is not this one.
        if host.world.pending_named_scene_transition.is_some() {
            eprintln!("[ok] {scene}: scene change requested at tick {t}");
            break;
        }
    }
    (host, ops)
}

/// Does the id name a keyable row? Static ids index the 100-row
/// `DAT_8006F198` table; runtime ids the scene's own record 0.
fn resolves(host: &SceneHost, id: i16) -> bool {
    if (0..0x64).contains(&id) {
        return true;
    }
    host.world
        .runtime_sfx_descriptor(id)
        .is_some_and(|row| row[3] & 0x1F != 0)
}

/// Run one authored MAN record's bytecode through the scene's live field VM
/// and collect the ring ops it issues. The record is the disc's own bytes:
/// `partition_record_span` locates it and its first opcode exactly the way
/// the field-op census does.
fn run_record(
    extracted: &PathBuf,
    scene: &str,
    partition: usize,
    record: usize,
    ticks: u32,
) -> (SceneHost, Vec<SfxRingOp>) {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene(scene, 0).expect("enter scene");
    let man = host
        .scene
        .as_ref()
        .expect("scene loaded")
        .field_man_payload(&host.index)
        .expect("read MAN")
        .expect("scene carries a MAN");
    let man_file = man_section::parse(&man).expect("parse MAN");
    let (start, pc0, len) =
        census::partition_record_span(&man_file, &man, partition, record).expect("record span");
    let code = man[start + pc0..start + len].to_vec();
    host.world.load_field_script(code);
    let mut ops = Vec::new();
    for _ in 0..ticks {
        host.world.set_pad(0);
        let _ = host.world.tick();
        ops.extend(host.world.take_sfx_ring_ops());
    }
    (host, ops)
}

/// `opdeene`'s partition-2 record 6 (`36 00 80 2A 00` / `36 04 80 00 00`,
/// then `36 00 80 29 00` / `36 04 80 00 00`) and `town01`'s partition-0
/// record 1, run as authored: each pushes its ids in order, opdeene's pushes
/// are each followed by the delay write the script pairs with them, and every
/// id resolves to a keyable row.
#[test]
fn authored_records_push_their_scripted_cues() {
    let Some(extracted) = gate() else { return };
    let cases: [(&str, usize, usize, &[SfxRingOp]); 2] = [
        (
            "opdeene",
            2,
            6,
            &[
                SfxRingOp::Push(0x2A),
                SfxRingOp::SetLastDelay(0),
                SfxRingOp::Push(0x29),
                SfxRingOp::SetLastDelay(0),
            ],
        ),
        // town01's partition-0 record 1 branches on system flag `0x232`
        // first; on a fresh world it takes the arm that pushes `0x2E` and
        // opens a text box (the locked-door `0x1C` pair is the other arm).
        ("town01", 0, 1, &[SfxRingOp::Push(0x2E)]),
    ];
    for (scene, partition, record, want) in cases {
        let (host, ops) = run_record(&extracted, scene, partition, record, 600);
        eprintln!("[ok] {scene} p{partition}[{record}] ring ops: {ops:x?}");
        assert!(
            ops.len() >= want.len() && ops[..want.len()] == *want,
            "{scene} p{partition}[{record}] issues {want:x?} first, got {ops:x?}"
        );
        for op in &ops {
            if let SfxRingOp::Push(id) = op {
                assert!(resolves(&host, *id), "{scene} cue {id:#x} resolves");
            }
        }
    }
}

/// Every op-`0x36` sub-`0` cue id a scene's own MAN scripts, resolved against
/// that scene's own bank: static ids against the 100-row table, runtime ids
/// (`>= 0x200`) against the scene's prescript record 0. One id per scene is
/// also pushed through the live field VM to show the path end to end.
#[test]
fn every_scripted_cue_resolves_in_its_own_scene() {
    let Some(extracted) = gate() else { return };
    let names = SceneHost::open_extracted(&extracted)
        .expect("open SceneHost")
        .index
        .cdname_scene_names();
    let (mut sites, mut runtime_sites, mut scenes) = (0usize, 0usize, 0usize);
    let mut motion_sites = 0usize;
    let mut unresolved: Vec<(String, i16)> = Vec::new();
    let mut silent: Vec<(String, i16)> = Vec::new();
    for scene in &names {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        if host.enter_field_scene(scene, 0).is_err() {
            continue;
        }
        let Ok(Some(man)) = host
            .scene
            .as_ref()
            .expect("scene")
            .field_man_payload(&host.index)
        else {
            continue;
        };
        let Ok(man_file) = man_section::parse(&man) else {
            continue;
        };
        let mut ids = Vec::new();
        for (_p, _r, start, pc0, len) in census::man_script_spans(&man_file, &man) {
            let body = &man[start..start + len];
            for step in census::LinearWalker::new(body, pc0) {
                let Ok(insn) = step else { break };
                if let census::InsnInfo::SceneFade {
                    word0: 0x8000,
                    word1,
                } = insn.info
                {
                    ids.push(word1 as i16);
                }
            }
        }
        // The motion VM's op `0x09` sites (MAN tail-section 1), walked by
        // op width the way `ambient_motion_disc_oracle` walks them.
        for rec in legaia_asset::man_motion::motion_records(&man, &man_file) {
            for var in legaia_asset::man_motion::stream_variants(&man, &rec) {
                let (a, b) = (var.code_offset, var.code_end.min(man.len()));
                let mut pc = a;
                while pc < b {
                    let Some(w) = legaia_asset::man_motion::op_width(man[pc]) else {
                        break;
                    };
                    if man[pc] == 0x09 && pc + 3 <= b {
                        ids.push(i16::from_le_bytes([man[pc + 1], man[pc + 2]]));
                        motion_sites += 1;
                    }
                    pc += w;
                }
            }
        }
        if ids.is_empty() {
            continue;
        }
        scenes += 1;
        sites += ids.len();
        for &id in &ids {
            if id >= 0x200 {
                runtime_sites += 1;
            }
            if !resolves(&host, id) {
                // A row that reads but keys no voice is a runtime id past the
                // end of record 0: the drainer reads the next record's header
                // as a descriptor, whose count byte is zero, and keys nothing.
                if host.world.runtime_sfx_descriptor(id).is_some() {
                    silent.push((scene.clone(), id));
                } else {
                    unresolved.push((scene.clone(), id));
                }
            }
        }
        // One id, end to end through the field VM.
        let id = ids[0];
        let [lo, hi] = (id as u16).to_le_bytes();
        host.world
            .load_field_script(vec![0x36, 0x00, 0x80, lo, hi, 0x00]);
        let _ = host.world.tick();
        assert_eq!(
            host.world.take_sfx_ring_ops(),
            vec![SfxRingOp::Push(id)],
            "{scene}: the VM pushes its scripted id"
        );
    }
    unresolved.sort();
    unresolved.dedup();
    silent.sort();
    silent.dedup();
    eprintln!(
        "[ok] {sites} cue sites ({runtime_sites} runtime-bank, {motion_sites} motion-VM op 0x09) \
         across {scenes} scene MANs; \
         unresolved {unresolved:x?}; retail-silent {silent:x?}"
    );
    assert!(sites > 0 && runtime_sites > 0, "the census found sites");
    // `balden2`'s record 0 carries 11 rows (`0x200..=0x20A`) and its scripts
    // push `0x20B`: the one authored id on the disc that keys nothing.
    assert_eq!(silent, vec![("balden2".to_string(), 0x20B)]);
    assert!(
        unresolved.is_empty(),
        "scripted cue ids no row backs: {unresolved:x?}"
    );
}

#[test]
fn opdeene_prescript_raises_the_frame_step_floor_to_three() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("opdeene", 0).expect("enter opdeene");
    let mut ticks = 0u32;
    while !host.world.cutscene_narration_active() && ticks < 2_000 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        ticks += 1;
    }
    assert!(host.world.cutscene_narration_active(), "narration opens");
    eprintln!(
        "[ok] opdeene at narration open (tick {ticks}): DAT_8007B9D8 = {}, floor = {}, step = {}",
        host.world.move_vm.dat_8007b9d8,
        host.world.clock.frame_step_floor,
        host.world.clock.frame_step
    );
    assert_eq!(host.world.move_vm.dat_8007b9d8, 3);
    assert_eq!(host.world.clock.frame_step_floor, 3);
    assert_eq!(host.world.clock.frame_step, 3);
}

/// Which scenes push cues with no input, and that every id they push
/// resolves. Every CDNAME scene that enters as a field scene is driven for up
/// to 4000 ticks (or to its scripted scene change) and the census is printed;
/// any id a scene pushes must resolve to a descriptor row.
#[test]
fn field_scenes_push_resolvable_cue_ids() {
    let Some(extracted) = gate() else { return };
    let names = SceneHost::open_extracted(&extracted)
        .expect("open SceneHost")
        .index
        .cdname_scene_names();
    let mut pushing = Vec::new();
    let mut entered = 0usize;
    for scene in &names {
        let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
        if host.enter_field_scene(scene, 0).is_err() {
            continue;
        }
        entered += 1;
        drop(host);
        let (host, ops) = ring_ops(&extracted, scene, 4_000);
        let mut ids: Vec<i16> = ops
            .iter()
            .filter_map(|(_, o)| match o {
                SfxRingOp::Push(id) | SfxRingOp::ReplaceLast(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            continue;
        }
        let bad: Vec<i16> = ids
            .iter()
            .copied()
            .filter(|id| !resolves(&host, *id))
            .collect();
        eprintln!(
            "[ok] {scene}: {} ring op(s), ids {:x?}, unresolved {:x?}",
            ops.len(),
            ids,
            bad
        );
        assert!(
            bad.is_empty(),
            "{scene} pushes ids no descriptor row backs: {bad:x?}"
        );
        pushing.push(scene.clone());
    }
    eprintln!(
        "[ok] {} of {entered} entered field scenes push a cue unprompted: {pushing:?}",
        pushing.len()
    );
    // The census, not a floor: on the retail disc no scene reaches a producer
    // unprompted, so what this pins is `entered` (the sweep really ran) and
    // the resolvability of whatever does get pushed.
    assert!(
        entered > 90,
        "the sweep entered the field scenes ({entered})"
    );
}
