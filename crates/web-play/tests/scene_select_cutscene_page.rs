//! Disc-gated: the play page's scene list opens the selector's story-beat
//! scenes as the native window's `--scene` entry does - the same seat, the
//! same flags raised, the same beat engaging the player on the first ticks.
//!
//! Both hosts stage the pick (`World::stage_picker_entry`) and arm it with
//! `SceneHost::arm_picker_story_entry`; the scenes are one per way the
//! entry-beat table starts a beat (`BeatTrigger`): an arrival tile (`bubu1`,
//! `bubu2`, the latter with C2 flags raised), the entry script behind the
//! door's hand-off flags (`jouine`, `urudre2`) and a trigger past the
//! arrival (`juui2`).
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::world::World;
use legaia_web_play::runtime::LegaiaRuntime;

const SCENES: [&str; 5] = ["bubu1", "bubu2", "jouine", "urudre2", "juui2"];

/// What a host's entry produced: the player's seat, the beat flags raised,
/// and the tick (of the first four) the beat engaged the player on.
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    seat: (i16, i16),
    flags: Vec<u16>,
    engaged: Option<u32>,
}

fn seat(w: &World) -> (i16, i16) {
    let slot = w.player_actor_slot.expect("player installed") as usize;
    let ms = &w.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

#[test]
fn the_page_and_the_window_open_the_same_cutscene() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    for scene in SCENES {
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes.clone(), String::new())
            .expect("page loads the disc");
        rt.enter_field(scene).expect("page entry");
        let page = {
            let w = &rt.debug_session().expect("session").host.world;
            (seat(w), w.field_vm.picker_beat_flags.clone())
        };
        let mut engaged = None;
        for t in 0..4u32 {
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
            let w = &rt.debug_session().expect("session").host.world;
            if w.script_context_engages_player() {
                engaged = Some(t);
                break;
            }
        }
        let page = Entry {
            seat: page.0,
            flags: page.1,
            engaged,
        };

        // The native window's `--scene` entry (`enter_launch_scene`).
        let cfg = legaia_engine_session::session::BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let mut s = legaia_engine_session::native::NativeSession::open_disc_detached_audio(
            std::path::Path::new(&disc),
            &cfg,
        )
        .expect("native session");
        s.host.world.stage_picker_entry(scene, false);
        s.host.arm_picker_story_entry(scene);
        s.enter_field_live(scene, &Default::default())
            .expect("native entry");
        let native_seat = seat(&s.host.world);
        let native_flags = s.host.world.field_vm.picker_beat_flags.clone();
        let mut engaged = None;
        for t in 0..4u32 {
            s.host.world.set_pad(0);
            let _ = s.tick();
            if s.host.world.script_context_engages_player() {
                engaged = Some(t);
                break;
            }
        }
        let native = Entry {
            seat: native_seat,
            flags: native_flags,
            engaged,
        };
        eprintln!("{scene}: page {page:?} native {native:?}");
        assert!(
            !page.flags.is_empty() || scene == "juui2",
            "{scene}: flags raised"
        );
        assert!(page.engaged.is_some(), "{scene}: the page's beat starts");
        assert_eq!(page, native, "{scene}: the page opens it unlike the window");
    }
}
