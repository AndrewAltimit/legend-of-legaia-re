//! `legaia-engine entry-bench`: what a scene and a battle cost to enter, with no
//! window and no GPU, for `scripts/ci/frame-bench.py --entry`. A window's own
//! steps past these (mesh upload, draw lists) are the same driver's startup
//! table.
//!
//! Each scene is entered through the scene picker's path, then again as a door
//! would (nothing armed for it), so the first row carries the once-per-process
//! caches and the second what a door back into the scene costs.

use super::*;
use legaia_engine_session::session::FieldLiveOpts;
use std::time::Instant;

fn step<T>(scene: &str, name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let r = f();
    println!(
        "[entry] {scene} {name} {:.2}",
        t.elapsed().as_secs_f64() * 1e3
    );
    r
}

/// Enter every scene of `scenes` in order, `repeat` times over, timing each
/// step; a `[entry] <scene> <step> <ms>` line per step.
pub fn cmd_entry_bench(extracted_root: &Path, scenes: &[String], repeat: u32) -> Result<()> {
    let cfg = legaia_engine_session::BootConfig {
        scene: String::new(),
        enable_audio: true,
    };
    let mut session = step("-", "open", || {
        NativeSession::open_extracted_detached_audio(extracted_root, &cfg)
    })?;
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    for round in 0..repeat.max(1) {
        for scene in scenes {
            let label = if round == 0 {
                scene.clone()
            } else {
                format!("{scene}+")
            };
            if round == 0 {
                step(&label, "picker seat", || {
                    session.host.world.stage_picker_entry(scene, false);
                    session.host.arm_picker_entry_seat(scene);
                });
            }
            step(&label, "host entry", || {
                session.enter_scene_live(scene, &opts)
            })?;
            step(&label, "window resources", || {
                build_window_scene_resources(&session)
            })?;
            let Some(id) = session.host.world.first_rollable_formation_id() else {
                continue;
            };
            if !session.host.world.force_encounter(id) {
                continue;
            }
            // The swirl's ticks, then the edge's: the longest tick is the load.
            let mut worst = 0f64;
            for _ in 0..600 {
                let t = Instant::now();
                session.tick()?;
                worst = worst.max(t.elapsed().as_secs_f64() * 1e3);
                if session.host.world.mode == legaia_engine_mode::mode::SceneMode::Battle {
                    break;
                }
            }
            println!("[entry] {label} battle worst tick {worst:.2}");
            if session.host.world.mode != legaia_engine_mode::mode::SceneMode::Battle {
                continue;
            }
            let archive = session
                .host
                .index
                .entry_bytes(legaia_engine_session::battle_stage::MONSTER_ARCHIVE_PROT_INDEX)
                .ok();
            step(&label, "battle stage", || {
                legaia_engine_session::battle_stage::stage_battle_world(
                    &mut session.host,
                    None,
                    archive.as_deref().map(Vec::as_slice),
                )
            });
            step(&label, "battle forms", || {
                session.host.ensure_battle_party_forms()
            });
        }
    }
    Ok(())
}
