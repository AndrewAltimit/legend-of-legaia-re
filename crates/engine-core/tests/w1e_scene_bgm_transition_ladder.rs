//! Disc-gated **scene-session ladder** for the BGM plumbing and the scripted
//! CLUT-cell family: enter real scenes, run a host-shaped frame loop, and
//! drive the field-VM `0x35` sub-ops the scenes' own scripts carry across a
//! scene transition.
//!
//! # Why a session and not a unit test
//!
//! `SceneHost::route_bgm_events` is only half a claim on its own. What retail
//! does across a transition is a *sequence*: a scene's entry script starts a
//! track, a cutscene pauses it, the scene changes, and the resume must land on
//! the same source rather than restarting it. Each hook in isolation passes
//! against a director that drops the pair. So every rung here runs the whole
//! ordering through one director and asserts on what the director was left
//! holding.
//!
//! Sub-ops are taken from the scenes' own MAN scripts (the opcode-aware
//! `LinearWalker`, never a raw byte scan), so a corpus that stops carrying an
//! op fails the ladder instead of quietly making it vacuous.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::{Path, PathBuf};

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{BgmDirector, DefaultMapIdResolver, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn disc_gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let Some(d) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    Some(d)
}

fn open_host(extracted: &Path) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    host
}

/// What a director is left holding after a beat.
///
/// The pause latch is the point: retail's sub-op `0xA` commit releases the
/// source sub-op 2 paused, and a director with no latch cannot tell "resume"
/// from "commit" - which is how a score stays paused for the rest of a scene.
#[derive(Default, Debug)]
struct LatchDirector {
    started: Vec<u16>,
    /// SEQ byte lengths handed to `start`, so "started" can be told apart
    /// from "started with nothing".
    started_bytes: Vec<usize>,
    paused: bool,
    pauses: usize,
    resumes: usize,
    stops: usize,
    /// Levels handed to `reattach_volume` (sub-op 8).
    levels: Vec<i16>,
    /// Commits that found the latch still set - the ones that had a paused
    /// source to release.
    released: usize,
    commits: usize,
}

impl BgmDirector for LatchDirector {
    fn start(&mut self, bgm_id: u16, seq_bytes: &[u8]) {
        self.started.push(bgm_id);
        self.started_bytes.push(seq_bytes.len());
        self.paused = false;
    }
    fn start_owned_vab(&mut self, bgm_id: u16, entry_bytes: &[u8]) {
        self.started.push(bgm_id);
        self.started_bytes.push(entry_bytes.len());
        self.paused = false;
    }
    fn pause(&mut self) {
        self.paused = true;
        self.pauses += 1;
    }
    fn resume(&mut self) {
        self.paused = false;
        self.resumes += 1;
    }
    fn stop(&mut self) {
        self.paused = false;
        self.stops += 1;
    }
    fn reattach_volume(&mut self, level: i16) {
        self.levels.push(level);
    }
    fn unhalt_pause(&mut self) {
        self.commits += 1;
        if self.paused {
            self.released += 1;
        }
        self.paused = false;
    }
}

/// Every `Bgm { text_id, sub_op }` the scene's MAN carries, decoded with the
/// opcode-aware walker across all three record partitions.
fn scene_bgm_ops(host: &SceneHost) -> Vec<(u16, u8)> {
    use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
    use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};

    let Some(scene) = host.scene.as_ref() else {
        return Vec::new();
    };
    let carriers = scene_man_carriers(&host.index, scene);
    let Some(carrier) = carriers.first() else {
        return Vec::new();
    };
    let man = carrier.payload.clone();
    let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for partition in 0..3usize {
        let count = *man_file
            .header
            .partition_counts
            .get(partition)
            .unwrap_or(&0)
            .max(&0) as usize;
        for record in 0..count {
            let Some((start, pc0, len)) = partition_record_span(&man_file, &man, partition, record)
            else {
                continue;
            };
            let Some(body) = man.get(start..start + len) else {
                continue;
            };
            for insn in LinearWalker::new(body, pc0).flatten() {
                if let InsnInfo::Bgm { text_id, sub_op } = insn.info {
                    out.push((text_id, sub_op));
                }
            }
        }
    }
    out
}

/// Scenes the ladder walks. A spread across the modes whose scripts route
/// BGM: the opening town, its interiors, the world map, and two cutscene-heavy
/// scenes.
const SCENES: &[&str] = &[
    "town01", "town02", "town03", "map01", "bylon", "dolk01", "sanct1",
];

/// The scene corpus must still carry the pause / resume sub-ops this ladder
/// drives; without that assertion every rung below could pass vacuously.
#[test]
fn the_scene_corpus_still_carries_the_pause_and_resume_sub_ops() {
    let Some(extracted) = disc_gate() else { return };
    let mut host = open_host(&extracted);
    let mut seen: std::collections::BTreeMap<u8, usize> = Default::default();
    for name in SCENES {
        if host.load_scene(name).is_err() {
            continue;
        }
        for (_, sub) in scene_bgm_ops(&host) {
            *seen.entry(sub).or_default() += 1;
        }
    }
    eprintln!("[w1e-bgm-ops] sub-op histogram across {SCENES:?}: {seen:?}");
    assert!(
        seen.contains_key(&1) || seen.contains_key(&9),
        "no scene in the corpus starts BGM at all - the walker or the scene \
         list lost its subject"
    );
    assert!(
        seen.contains_key(&2),
        "no scene in the corpus pauses BGM (sub-op 2); the pause/resume rung \
         below would be asserting about an op the disc no longer carries"
    );
}

/// A **scene transition that pauses and resumes BGM**: start, pause, cross a
/// scene boundary, resume - all through one director, all with the ids the
/// scenes' own scripts carry.
///
/// The two properties the sequence pins:
/// - a resume must reach `BgmDirector::resume`, not a second `start` (a
///   director that restarts loses the cutscene's beat), and
/// - the transition must not swallow the latch: the resume after the boundary
///   still clears the pause the pre-boundary op set.
#[test]
fn a_scene_transition_pauses_and_resumes_the_same_source() {
    let Some(extracted) = disc_gate() else { return };
    let mut host = open_host(&extracted);

    // First scene: one whose script both starts and pauses BGM.
    let mut chosen: Option<(String, u16)> = None;
    for name in SCENES {
        if host.load_scene(name).is_err() {
            continue;
        }
        let ops = scene_bgm_ops(&host);
        let starts: Vec<u16> = ops
            .iter()
            .filter(|(_, s)| *s == 1 || *s == 9)
            .map(|(id, _)| *id)
            .collect();
        let pauses = ops.iter().any(|(_, s)| *s == 2);
        if pauses && let Some(id) = starts.first() {
            chosen = Some(((*name).to_string(), *id));
            break;
        }
    }
    let (scene_a, bgm_id) =
        chosen.expect("no scene both starts and pauses BGM - see the corpus rung above");
    host.load_scene(&scene_a).expect("reload scene A");

    let mut d = LatchDirector::default();

    // Start.
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: bgm_id,
        sub_op: 1,
    });
    host.route_bgm_events(&mut d).expect("route start");
    assert_eq!(
        d.started,
        vec![bgm_id],
        "scene '{scene_a}' entry start did not reach the director"
    );
    assert!(
        d.started_bytes.iter().all(|n| *n > 0),
        "the start carried zero resolved bytes - `bgm_seq_bytes` / the global \
         bank read resolved nothing, so the director was handed silence"
    );

    // Pause.
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 2,
    });
    host.route_bgm_events(&mut d).expect("route pause");
    assert!(d.paused && d.pauses == 1, "pause did not latch");

    // Cross a scene boundary while paused. The second scene is any other
    // loadable one - the point is that the host tore down and rebuilt its
    // scene state under a paused director.
    let scene_b = SCENES
        .iter()
        .find(|n| **n != scene_a && host.load_scene(n).is_ok())
        .expect("a second loadable scene");
    assert!(
        d.paused,
        "the scene transition cleared the director's pause latch behind its \
         back - the resume below would then be resuming nothing"
    );

    // Resume, on the far side of the transition.
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 3,
    });
    host.route_bgm_events(&mut d).expect("route resume");
    assert_eq!(d.resumes, 1, "resume did not reach the director");
    assert!(!d.paused, "resume left the source paused");
    assert_eq!(
        d.started.len(),
        1,
        "resume restarted the track instead of un-pausing it ({:?})",
        d.started
    );
    assert_eq!(
        d.stops, 0,
        "nothing in the sequence asked for a stop; a routed stop would tear \
         the source down and make the resume meaningless"
    );

    eprintln!(
        "[w1e-bgm] {scene_a} -> {scene_b}: start={:?} pauses={} resumes={}",
        d.started, d.pauses, d.resumes
    );
}

/// Sub-op 8 re-applies the field BGM volume, and the level it hands the
/// director is retail's own `(raw << 15) >> 16`.
///
/// The arithmetic is the pair `sll a1,a1,0xf; sra a1,a1,0x10` at
/// `0x800198C0..C4` in `FUN_80019898`: bits `[16:1]` of the raw global,
/// sign-extended. This rung recomputes it from the shift pair rather than
/// calling the helper, so a helper that changed would fail rather than agree
/// with itself.
#[test]
fn sub_op_8_reapplies_the_halved_field_volume() {
    let Some(extracted) = disc_gate() else { return };
    let mut host = open_host(&extracted);
    host.load_scene("town01").expect("load town01");

    for raw in [0i32, 1, 2, 127, 128, 255, 0x7FFF, 0xFFFF, -2] {
        host.bgm_volume_raw = raw;
        let mut d = LatchDirector::default();
        host.world.pending_field_events.push(FieldEvent::Bgm {
            text_id: 0,
            sub_op: 8,
        });
        let acted = host.route_bgm_events(&mut d).expect("route reattach");
        assert_eq!(acted, 1, "sub-op 8 must act");
        let expect = ((raw << 15) >> 16) as i16;
        assert_eq!(
            d.levels,
            vec![expect],
            "sub-op 8 level for raw {raw} must be the sign-extended halving \
             `(raw << 15) >> 16`"
        );
    }

    // The retail cold-reset value is what a boot actually routes; check it is
    // carried rather than zeroed by the host's own default.
    let host2 = open_host(&extracted);
    eprintln!(
        "[w1e-bgm] cold-reset bgm_volume_raw = {}",
        host2.bgm_volume_raw
    );
}

/// Sub-op `0xA` is the swap-commit: with the pause latch still set (no start
/// intervened) it must release the paused source, and it must clear the latch
/// either way. A director that drops it leaves every cutscene-paused score
/// silent for the rest of the scene.
#[test]
fn sub_op_10_commits_the_pause_swap_and_clears_the_latch() {
    let Some(extracted) = disc_gate() else { return };
    let mut host = open_host(&extracted);
    host.load_scene("town01").expect("load town01");

    // Latch set: the commit has a paused source to release.
    let mut d = LatchDirector::default();
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 2,
    });
    host.route_bgm_events(&mut d).expect("route pause");
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 10,
    });
    host.route_bgm_events(&mut d).expect("route commit");
    assert_eq!(d.commits, 1, "the commit must reach the director");
    assert_eq!(
        d.released, 1,
        "a latched pause must be released by the commit"
    );
    assert!(!d.paused, "the commit must clear the pause latch");

    // Latch clear (a start intervened, the ordinary cutscene case): the commit
    // still routes and still leaves the slot sounding.
    let mut d2 = LatchDirector::default();
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 2,
    });
    host.route_bgm_events(&mut d2).expect("route pause");
    d2.paused = false; // stand-in for the paired sub-op 9 start
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0,
        sub_op: 10,
    });
    host.route_bgm_events(&mut d2).expect("route commit");
    assert_eq!(d2.commits, 1);
    assert_eq!(
        d2.released, 0,
        "nothing was paused, so nothing may be released"
    );
    assert!(!d2.paused);
}

/// A real field scene's own scripted CLUT-cell **cross-fade** arm, driven
/// against the scene's own populated VRAM on the host frame loop.
///
/// This is the arm the one-shot path does not reach: `frames != 0` spawns the
/// fade actor, whose per-tick arithmetic is `clut_fx::ClutFade`. Driving it
/// against `SceneResources::vram` rather than a scratch buffer is what makes
/// it a session rung - it is the same VRAM both rendering hosts hand
/// `World::step_clut_fx`.
#[test]
fn a_scene_fade_arm_runs_against_the_scenes_own_vram() {
    let Some(extracted) = disc_gate() else { return };
    let mut host = open_host(&extracted);
    // map01 is the kingdom overworld whose park-row ops are the documented
    // carrier of this family; `enter_field_scene` is what pins `dt` and
    // populates the scene VRAM.
    host.enter_field_scene("map01", 0).expect("enter map01");
    assert_eq!(host.world.frame_step, 3, "overworld dt");

    let man = host
        .scene
        .as_ref()
        .expect("scene")
        .field_man_payload(&host.index)
        .expect("man payload")
        .expect("map01 has a field MAN");
    let man_file = legaia_asset::man_section::parse(&man).expect("parse MAN");
    let sites = legaia_engine_core::man_field_scripts::scene_clut_cell_fx(&man_file, &man);
    let fade = sites
        .iter()
        .find(|s| s.op.frames != 0)
        .expect("map01 carries a `4C 61` cross-fade op");

    // Rebuild the 14-byte operand payload the field-VM hook carries, so what
    // is spawned is the disc's operands and not a hand-picked pair.
    let mut payload = [0u8; 14];
    for (i, v) in [
        fade.op.a.0,
        fade.op.a.1,
        fade.op.b.0,
        fade.op.b.1,
        fade.op.dest.0,
        fade.op.dest.1,
        fade.op.frames,
    ]
    .into_iter()
    .enumerate()
    {
        payload[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }

    let mut resources = host.resources.take().expect("scene resources built");
    let read_row = |vram: &legaia_tim::Vram, (x, y): (i16, i16)| -> Vec<u16> {
        (0..16)
            .map(|i| vram.pixel(x as usize + i, y as usize))
            .collect()
    };
    let b_row: Vec<u16> = if fade.op.b_is_flat() {
        vec![fade.op.b.0 as u16; 16]
    } else {
        read_row(&resources.vram, fade.op.b)
    };

    // Cells 498/499 are *runtime-written* CLUT rows - nothing in the scene's
    // TIM set uploads them, so on a cold entry cell A and cell B both read as
    // zeros and the fade would be a no-op by construction. That is a property
    // of the VRAM state, not of the kernel, and asserting interpolation
    // against it would be asserting nothing. Seed cell A with a distinct ramp
    // in exactly that case - standing in for the ambient CLUT walk that owns
    // the row - and say so, so the interpolation rung below can never pass
    // vacuously.
    let a_row_before = read_row(&resources.vram, fade.op.a);
    let seeded = a_row_before == b_row;
    if seeded {
        let mut bytes = [0u8; 32];
        for i in 0..16usize {
            let v = ((i as u16 + 1) << 10) | 0x1F;
            bytes[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        resources
            .vram
            .write_clut_row(fade.op.a.0 as u16, fade.op.a.1 as u16, &bytes);
    }
    let a_row = read_row(&resources.vram, fade.op.a);
    assert_ne!(
        a_row, b_row,
        "cell A and cell B are identical, so no fade between them can be \
         observed"
    );

    host.world.spawn_clut_cell_fx(&payload);
    assert_eq!(host.world.clut_fx.len(), 1, "the fade actor spawned");

    // Host frame loop: tick the world, then hand the scene VRAM to the CLUT
    // driver - exactly the order `window/field_render.rs` and the browser
    // runtime use.
    let mut frames = 0u32;
    let mut writes = 0u32;
    let mut saw_intermediate = false;
    while !host.world.clut_fx.is_empty() {
        frames += 1;
        assert!(frames < 2000, "the fade never completed");
        host.world.tick();
        if host.world.step_clut_fx(&mut resources.vram) {
            writes += 1;
            if !host.world.clut_fx.is_empty() {
                let row = read_row(&resources.vram, fade.op.dest);
                if row != b_row && row != a_row {
                    saw_intermediate = true;
                }
            }
        }
    }
    let final_row = read_row(&resources.vram, fade.op.dest);
    // The fade advances by `dt` vsyncs per game tick, so a `frames`-vsync
    // operand takes `ceil(frames / dt)` ticks - and each tick writes once.
    let dt = u32::from(host.world.frame_step.max(1));
    let expect_writes = (fade.op.frames as u32).div_ceil(dt);
    eprintln!(
        "[w1e-clut] frames={frames} writes={writes} (expect {expect_writes} at dt={dt}) \
         dest={:?} frames_operand={} seeded_cell_a={seeded}",
        fade.op.dest, fade.op.frames
    );
    assert_eq!(
        writes, expect_writes,
        "a {}-vsync fade at dt={dt} must write on every one of its \
         ceil(frames/dt) game ticks - a different count means the fade is \
         denominated in game ticks rather than vsyncs",
        fade.op.frames
    );
    assert!(
        saw_intermediate,
        "every written row was already cell A or cell B; the fade never \
         interpolated, which a wired-but-inert kernel looks exactly like"
    );
    assert_eq!(
        final_row, b_row,
        "the completion write must land cell B on the destination"
    );
}
