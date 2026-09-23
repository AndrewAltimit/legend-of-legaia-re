//! Disc-gated: which scene scripts raise the field fog gate, and that the
//! native engine draws the fog those scenes ask for.
//!
//! The fog pool (`legaia_engine_core::fog_particles`) draws nothing until a
//! script raises `_DAT_8007B854` through field-VM op `0x4C` nibble-3 sub-0
//! (`[4C, 0x30]`, `0x801E0F38`); sub-1 (`[4C, 0x31]`) clears it. This file
//! is the disc-wide census of both sites over every scene MAN's field-VM
//! bytecode - the denominator behind "where does retail show fog" - and,
//! for the first scene whose entry script raises the gate, the oracle: boot
//! it natively, run until the gate is up, and assert the render step emits
//! retail's packet shape (texpage `0x27`, CLUT `0x7640`, the two staged UV
//! rows, two quads per drawn particle) with the effect-atlas cells resident
//! in the scene VRAM the quads sample.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use legaia_engine_core::camera_view::{FieldCameraFrame, resolve_field_camera};
use legaia_engine_core::fog_particles::{
    FOG_CAP_DEFAULT, FOG_CLUT, FOG_LEFT_UV_ROW, FOG_RIGHT_UV_ROW, FOG_TPAGE, FOG_UV_ROWS,
};
use legaia_engine_core::man_field_scripts::{partition_record_span, scene_man_carriers};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_vm::field_disasm::{InsnInfo, LinearWalker};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One decoded gate site.
#[derive(Debug, Clone)]
struct GateSite {
    scene: String,
    partition: usize,
    record: usize,
    abs_pc: usize,
    /// `true` for sub-0 (raise), `false` for sub-1 (clear).
    raise: bool,
    /// No `SystemFlag` test, `BBoxTest` or `CondJmp` precedes the site in
    /// its record: the op runs whenever the record does.
    unconditional: bool,
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

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })
}

fn census(index: &ProtIndex) -> Vec<GateSite> {
    let mut out = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            let partitions = man_file.header.partition_counts.len();
            for partition in 0..partitions {
                let records = man_file.header.partition_counts[partition].max(0) as usize;
                for record in 0..records {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut conditional_seen = false;
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        match &insn.info {
                            InsnInfo::SystemFlag {
                                kind: legaia_engine_vm::field_disasm::FlagKind::Test,
                                ..
                            }
                            | InsnInfo::BBoxTest { .. }
                            | InsnInfo::CondJmp { .. } => conditional_seen = true,
                            _ => {}
                        }
                        let InsnInfo::MenuCtrl { op0, .. } = insn.info else {
                            continue;
                        };
                        if insn.extended.is_some() || !(op0 == 0x30 || op0 == 0x31) {
                            continue;
                        }
                        out.push(GateSite {
                            scene: name.clone(),
                            partition,
                            record,
                            abs_pc: start + insn.pc,
                            raise: op0 == 0x30,
                            unconditional: !conditional_seen,
                        });
                    }
                }
            }
        }
    }
    out
}

#[test]
fn fog_gate_census_names_the_scenes_that_raise_it() {
    let Some(extracted) = gated() else { return };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let sites = census(&index);
    let mut by_scene: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for s in &sites {
        let e = by_scene.entry(s.scene.as_str()).or_default();
        if s.raise {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
        eprintln!(
            "[fog-gate census] {:<10} P{}[{:3}] @0x{:05X} {}{}",
            s.scene,
            s.partition,
            s.record,
            s.abs_pc,
            if s.raise { "RAISE" } else { "clear" },
            if s.unconditional { "" } else { " (fenced)" }
        );
    }
    for (scene, (raise, clear)) in &by_scene {
        eprintln!("[fog-gate census] {scene}: {raise} raise, {clear} clear");
    }
    eprintln!(
        "[fog-gate census] {} site(s) across {} scene(s) (of {} CDNAME scenes)",
        sites.len(),
        by_scene.len(),
        index.cdname_scene_names().len()
    );
    // Non-vacuity: the disc does raise the gate somewhere, else the whole
    // fog system would be dead code and the port's draw would be an
    // invention. If this fires, the walk framing changed - the op's arm is
    // pinned in the field-VM disassembly.
    assert!(
        sites.iter().any(|s| s.raise),
        "no `[4C, 0x30]` fog-gate raise in any scene MAN"
    );
}

/// The first scene whose scene-controller record (`P1[0]`, the placement
/// whose prologue the installer pre-runs at scene load) raises the gate
/// **unconditionally** - no story-flag test or player box fences it - so
/// the fog is up from entry without a walk-on or a dialogue. Every raise
/// the disc carries sits in a `P1` record; town01's, for one, is fenced by
/// three flag tests and a tile box, which is why "first raise" is not the
/// right pick.
fn entry_raising_scene(index: &ProtIndex) -> Option<String> {
    let sites = census(index);
    sites
        .iter()
        .find(|s| s.raise && s.unconditional && s.partition == 1 && s.record == 0)
        .map(|s| s.scene.clone())
        .or_else(|| {
            sites
                .iter()
                .find(|s| s.raise && s.unconditional)
                .map(|s| s.scene.clone())
        })
}

#[test]
fn native_engine_draws_retail_shaped_fog_once_the_script_raises_the_gate() {
    let Some(extracted) = gated() else { return };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let Some(scene) = entry_raising_scene(&index) else {
        panic!("census found no gate-raising scene");
    };
    eprintln!("[fog oracle] scene {scene}");
    let cfg = BootConfig {
        scene: scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    session
        .enter_field_live(&scene, &FieldLiveOpts::default())
        .expect("enter field scene");

    // The effect-atlas cells the fog samples are resident in the scene VRAM
    // from field entry, byte-for-byte what retail holds there. Texpage
    // `0x0027` is VRAM `(448, 0)` - bit 4, the Y-base bit, is clear; bit 5 is
    // ABR - and the two staged UV rows read `v 0x40..0x6F`, `u 0x00..0x3F`
    // (4bpp: sixteen halfwords). The CLUT `0x7640` is `(0, 473)`. Both
    // hashes are the retail ones `window/fog_texture_tests.rs` pins (nine
    // PCSX-Redux field / world-map states agree).
    {
        let res = session
            .host
            .resources
            .as_ref()
            .expect("scene resources after field entry");
        let fnv = |vals: &mut dyn Iterator<Item = u16>| {
            vals.fold(0xcbf2_9ce4_8422_2325u64, |h, p| {
                (h ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        };
        let cells = fnv(&mut (0x40..0x70)
            .flat_map(|y| (448..464).map(move |x| (x, y)))
            .map(|(x, y)| res.vram.pixel(x, y)));
        let clut = fnv(&mut (0..16).map(|x| res.vram.pixel(x, 473)));
        eprintln!("[fog oracle] fog cells {cells:016x}, clut {clut:016x}");
        assert_eq!(
            cells, 0xe9d8_110d_1eec_6070,
            "fog wisps at VRAM (448, 0x40..0x70) differ from retail"
        );
        assert_eq!(
            clut, 0x3e91_7d19_3b0e_dcf5,
            "fog CLUT at (0, 473) differs from retail"
        );
    }

    // Run the entry script until it raises the gate.
    let mut raised_at = None;
    for f in 0..600 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        if session.host.world.fog.gate {
            raised_at = Some(f);
            break;
        }
    }
    let raised_at = raised_at.expect("the scene's script raises the fog gate within 600 ticks");
    eprintln!("[fog oracle] gate raised at tick {raised_at}");
    assert_eq!(
        session.host.world.fog.cap, FOG_CAP_DEFAULT,
        "retail cap after field entry"
    );

    // Then run the render step every tick through the follow camera the
    // native window resolves with, until something draws.
    let mut first_draw = None;
    let mut max_quads = 0usize;
    let mut max_live = 0u16;
    for f in 0..1200 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        let frame = resolve_field_camera(&session.host.world, &session.camera, None, [0.0, 0.0]);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            panic!("no field camera frame at tick {f}");
        };
        let quads = session.host.world.fog_render_step(&view).to_vec();
        let live = session.host.world.fog.live;
        max_live = max_live.max(live);
        if !quads.is_empty() {
            first_draw.get_or_insert(f);
            max_quads = max_quads.max(quads.len());
            for q in &quads {
                assert_eq!(q.tpage, FOG_TPAGE);
                assert_eq!(q.clut, FOG_CLUT);
                let row = |r: usize| -> [(u8, u8); 4] {
                    FOG_UV_ROWS[r].map(|w| ((w & 0xFF) as u8, ((w >> 8) & 0xFF) as u8))
                };
                assert!(
                    q.uv == row(FOG_LEFT_UV_ROW) || q.uv == row(FOG_RIGHT_UV_ROW),
                    "uv {:?} is neither staged row",
                    q.uv
                );
                // Axis-aligned in POLY_FT4 order.
                assert_eq!(q.xy[0].1, q.xy[1].1);
                assert_eq!(q.xy[2].1, q.xy[3].1);
                assert_eq!(q.xy[0].0, q.xy[2].0);
                assert_eq!(q.xy[1].0, q.xy[3].0);
            }
        }
        assert!(
            live <= FOG_CAP_DEFAULT + 4 * 24,
            "live {live} exceeds what one tick of the emitter can add over the cap"
        );
    }
    eprintln!(
        "[fog oracle] first draw at tick {:?}; peak quads/frame {max_quads}; peak live {max_live} (cap {FOG_CAP_DEFAULT})",
        first_draw
    );
    assert!(
        first_draw.is_some(),
        "gate raised but no fog quad was emitted in 1200 ticks"
    );
}

/// Pool density against the retail `vell` poll.
///
/// `autorun_w1a_fog_pool_poll.lua` on the `vell_fog_field` state (1800
/// vsyncs standing at the `map01` entrance, gate raised, cap `0x18`, frame
/// step `DAT_1F800393 = 2` on every sample) reads the alive-record population
/// at 21..38, mean 25.9. The port once read 40..62: its emitter drew the raw
/// world LCG state, whose low four bits cycle with period 16, where retail's
/// BIOS `rand()` returns the state's high half - so the burst gate
/// (`rand & 0xF == 0`) failed on almost every frame and then passed on all 24
/// draws of one frame, dropping fifty-odd records into the pool at once past
/// the stale live count. This pins the per-tick spawn count to what one
/// retail frame can produce and the settled population to retail's band.
#[test]
fn vell_fog_density_tracks_the_retail_poll() {
    let Some(extracted) = gated() else { return };
    let scene = "vell".to_string();
    let cfg = BootConfig {
        scene: scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    session
        .enter_field_live(&scene, &FieldLiveOpts::default())
        .expect("enter vell");
    let mut samples = Vec::new();
    let mut max_spawn = 0usize;
    for f in 0..3000 {
        session.host.world.set_pad(0);
        let before = session.host.world.fog.allocated();
        let _ = session.tick();
        let spawned = session.host.world.fog.allocated().saturating_sub(before);
        let frame = resolve_field_camera(&session.host.world, &session.camera, None, [0.0, 0.0]);
        let (FieldCameraFrame::Follow(view) | FieldCameraFrame::Cutscene(view)) = frame else {
            panic!("no field camera frame at tick {f}");
        };
        let _ = session.host.world.fog_render_step(&view);
        if f >= 600 {
            samples.push(session.host.world.fog.live);
            max_spawn = max_spawn.max(spawned);
        }
    }
    assert!(
        session.host.world.fog.gate,
        "vell's entry script raises the gate"
    );
    let n = samples.len();
    let mean = samples.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let min = *samples.iter().min().unwrap();
    let max = *samples.iter().max().unwrap();
    eprintln!(
        "[fog density] vell over {n} ticks: live {min}..{max}, mean {mean:.1}; \
         max spawns in one tick {max_spawn} (retail poll: alive 21..38, mean 25.9, cap 24)"
    );
    // One retail frame: 24 burst draws, each a 1-in-16 gate for 1..4 spawns.
    // Fifty-plus in one tick is the degenerate-gate shape this guards.
    assert!(
        max_spawn <= 24,
        "{max_spawn} fog spawns in one tick - the burst gate is not drawing BIOS rand()"
    );
    assert!(
        (18.0..=34.0).contains(&mean),
        "vell fog population mean {mean:.1} is outside retail's band (25.9, cap 24)"
    );
}
