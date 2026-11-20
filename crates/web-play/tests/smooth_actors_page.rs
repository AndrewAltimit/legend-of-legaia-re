//! Disc-gated: the smooth field actors (`WorldToggles::smooth_actors`)
//! reach the browser play page, and the page's Smooth motion box
//! (`LegaiaRuntime::set_smooth_motion`) turns them off.
//!
//! `izumi`'s first-visit choreography walks Vahn to his mark and the hunters
//! about the spring, each body stepped once a game frame as retail does
//! (`FUN_8002519C`). The page reads the drawn bodies through
//! `player_transform` (the player's matrix) and the NPC pose pass
//! (`World::field_npc_drawn_pose`): with smooth motion on, some drawn body
//! moves on a vsync its stored body holds; with it off, every drawn body is
//! the stored one. The two runs share one pad schedule and the simulation
//! matches vsync for vsync (presentation only), as the native
//! `engine-session` twin asserts.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use legaia_web_play::runtime::LegaiaRuntime;

const PAD_CROSS: u16 = 0x4000;
const TICKS: u32 = 900;

fn disc_bytes() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(PathBuf::from(path)).ok()
}

/// One vsync: the simulation's fingerprint, then `(key, stored, drawn)` per
/// body (NPC slots, the player under `256`, read through the page export).
type Sample = (String, Vec<(u16, [f32; 3], [f32; 3])>);

fn run(bytes: &[u8], smooth: bool) -> Vec<Sample> {
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes.to_vec(), String::new())
        .expect("load_disc");
    rt.set_smooth_motion(smooth);
    rt.enter_field("izumi").expect("enter izumi");
    {
        let w = &rt.scene_host.host().expect("host").world;
        assert_eq!(w.toggles.smooth_actors, smooth, "the box reaches the world");
        assert_eq!(w.toggles.smooth_camera, smooth);
        assert_eq!(w.toggles.smooth_walk, smooth);
    }
    let mut out = Vec::new();
    for t in 0..TICKS {
        rt.set_pad(if t % 40 == 0 { PAD_CROSS } else { 0 });
        rt.tick_frame().expect("tick_frame");
        let drawn_player = rt.player_transform();
        let w = &rt.scene_host.host().expect("host").world;
        let mut npcs: Vec<_> = w.npcs.positions.iter().map(|(&k, &v)| (k, v)).collect();
        npcs.sort_unstable();
        let actors: Vec<_> = w
            .actors
            .iter()
            .filter(|a| a.active)
            .map(|a| {
                let m = &a.move_state;
                (m.world_x, m.world_y, m.world_z, m.render_26)
            })
            .collect();
        let sim = format!(
            "{} {:08x} {:?} {:?} {}",
            w.active_scene_label, w.rng_state, npcs, actors, w.field_pc
        );
        let mut bodies = Vec::new();
        for &(slot, (x, z)) in &npcs {
            let (Some(stored), Some(drawn)) = (
                w.field_npc_draw_pose(slot, (x, z)),
                w.field_npc_drawn_pose(slot, (x, z)),
            ) else {
                continue;
            };
            bodies.push((u16::from(slot), stored.pos, drawn.pos));
        }
        if let Some(slot) = w.player_actor_slot.map(usize::from) {
            let m = &w.actors[slot].move_state;
            let stored = [m.world_x, m.world_y, m.world_z].map(f32::from);
            bodies.push((
                256,
                stored,
                [drawn_player[0], drawn_player[1], drawn_player[2]],
            ));
        }
        out.push((sim, bodies));
    }
    out
}

/// Vsyncs on which a stored body held while its drawn body moved.
fn drawn_moves_on_held(samples: &[Sample]) -> usize {
    let mut n = 0;
    for pair in samples.windows(2) {
        for b in &pair[1].1 {
            let Some(p) = pair[0].1.iter().find(|q| q.0 == b.0) else {
                continue;
            };
            n += usize::from(b.1 == p.1 && b.2 != p.2);
        }
    }
    n
}

#[test]
fn the_play_page_draws_smooth_field_actors_or_skip() {
    let Some(bytes) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let on = run(&bytes, true);
    let off = run(&bytes, false);
    for (t, (a, b)) in on.iter().zip(&off).enumerate() {
        assert_eq!(a.0, b.0, "vsync {t}: smooth motion moved the simulation");
    }
    for (t, (_, bodies)) in off.iter().enumerate() {
        for (key, stored, drawn) in bodies {
            assert_eq!(
                stored, drawn,
                "vsync {t}: body {key} drawn off its stored spot"
            );
        }
    }
    let smooth = drawn_moves_on_held(&on);
    eprintln!(
        "[ran] smooth-on drawn-moves-on-held vsyncs: {smooth}, off: {}",
        drawn_moves_on_held(&off)
    );
    assert!(smooth > 0, "no drawn body moved between game frames");
}
