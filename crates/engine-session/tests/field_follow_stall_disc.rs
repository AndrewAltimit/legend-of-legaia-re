//! Disc-gated walk sweep for the follow camera's stall-then-snap: the focus
//! freezing while the player walks on, then jumping the distance it fell
//! behind once something lets go.
//!
//! Two field-tick orderings produced it, both on Rim Elm's maps:
//!
//! - **A touch the walk posts.** Retail's field tick `FUN_801D1344` reads the
//!   player's engaged bit once, ahead of the pad controller
//!   (`0x801D1694..0x801D16C0`, kept in `s3`), and the follow ease's branch
//!   at `0x801D17DC` uses that answer. Sliding along `town01`'s door prop at
//!   tile `(22, 18)` posts its auto-touch every step, and the run raises the
//!   bit after the walk; the port read the bit live at the camera step and
//!   skipped every ease along the door.
//! - **A record the walk spawns.** The stand-and-press beats (`town01`
//!   P2[12..14] and their `town0b..0e` twins) re-run every frame the player
//!   is on their tiles (`2E 13`) and end in the same slice unless the button
//!   is down. Spawned after the walk, a fresh timeline stood at every camera
//!   step, and the port counted it as a cutscene holding the shot.
//!
//! The sweep holds a direction through each beat (no button), as a player
//! walking past does, and asserts no frame the player walks free snaps the
//! focus more than half a tile.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn holds(s: &NativeSession) -> bool {
    let w = &s.host.world;
    w.cutscene_timeline_active() || w.dialogue_owns_input() || w.script_context_engages_player()
}

#[derive(Debug, Default)]
struct Sweep {
    /// Free frames whose focus jumped more than half a tile while the player
    /// moved less than a quarter.
    snaps: Vec<(u32, i32)>,
    /// Frames a just-spawned, unstepped timeline stood at the camera step.
    fresh_timelines: u32,
    /// Frames a prop run opened by the walk stood at the camera step.
    prop_runs: u32,
}

fn sweep(extracted: &std::path::Path, scene: &str) -> Option<Sweep> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    s.set_host_drains_queues(true);
    for t in 0..3000u32 {
        if t >= 30 && !holds(&s) {
            break;
        }
        s.host.world.set_pad(if t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().ok()?;
    }
    let (r, d, l, u) = (
        PadButton::Right.mask(),
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
    );
    let dirs = [r, d, l, u, r | u, l | d, u | l, d | r];
    let label = s.host.world.active_scene_label.clone();
    let mut out = Sweep::default();
    let mut prev: Option<([i32; 2], [i32; 2])> = None;
    for t in 0..4800u32 {
        if s.host.world.active_scene_label != label {
            break;
        }
        let step = (t / 90) as usize + (t / 720) as usize;
        s.host.world.set_pad(dirs[step % dirs.len()]);
        s.tick().ok()?;
        let w = &s.host.world;
        if w.cutscene_timeline_active() && !w.cutscene_timeline_holds_camera() {
            out.fresh_timelines += 1;
        }
        if w.dialog
            .inline
            .as_ref()
            .is_some_and(|i| i.prop_anchor.is_some())
        {
            out.prop_runs += 1;
        }
        let Some(slot) = w.player_actor_slot else {
            continue;
        };
        let a = &w.actors[slot as usize].move_state;
        let p = [i32::from(a.world_x), i32::from(a.world_z)];
        let g = &s.camera.globals.0;
        let f = [-g[6], -g[8]];
        let free = !w.cutscene_timeline_holds_camera()
            && s.camera.mover.is_none()
            && !s.camera.script_owns_focus();
        if free && let Some((pp, pf)) = prev {
            let df = (f[0] - pf[0]).abs().max((f[1] - pf[1]).abs());
            let dp = (p[0] - pp[0]).abs().max((p[1] - pp[1]).abs());
            if df > 64 && dp < 32 {
                out.snaps.push((t, df));
            }
        }
        prev = Some((p, f));
    }
    Some(out)
}

#[test]
fn the_follow_camera_keeps_up_through_rim_elm_touches_and_polls() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut fresh = 0;
    let mut props = 0;
    for scene in ["town01", "town0c", "town0d"] {
        let Some(sw) = sweep(&extracted, scene) else {
            continue;
        };
        eprintln!("FOLLOW {scene} {sw:?}");
        fresh += sw.fresh_timelines;
        props += sw.prop_runs;
        assert!(
            sw.snaps.is_empty(),
            "{scene}: the focus snapped on a free walking frame: {:?}",
            sw.snaps
        );
    }
    assert!(fresh > 0, "the sweep never crossed a stand-and-press band");
    assert!(props > 0, "the sweep never brushed a touch prop");
}
