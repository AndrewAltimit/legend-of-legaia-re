//! Disc-gated: the Mt. Rikuroa summit (`rikuroa`, issue #582) on the play
//! page, against the native window's own entry path.
//!
//! - **The lead.** From the scene picker, the page and the native `--scene`
//!   entry seat the same field leader and party list, with and without
//!   Noa's chapter flags raised ahead of the entry. The summit's entry
//!   script picks Noa only when the flags are up at entry time, so a
//!   comparison that raised them before the native entry and after the
//!   page's drew Noa on one host and Vahn on the other with no defect in
//!   either.
//! - **The crop under the summit beat.** Seated on the walk-on tile, the
//!   beat takes the camera on the arrival tick, before the zone follow
//!   camera ever composes. The cull view used to be published only while
//!   the follow camera was active, so the whole beat ran with no
//!   visible-tile crop and no actor cull: scenery retail hides drew around
//!   the Genesis Tree and every scripted actor drew wherever it stood.
//! - **The seat.** The page's debug seat latches the region box at the
//!   seated tile; a seat that kept the entry's box ran with no crop.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::world::World;
use legaia_engine_world_views::field_view_window::{CropFraming, field_view_cells};
use legaia_web_play::runtime::LegaiaRuntime;

/// Noa's chapter: flags a playthrough holds when the summit is reached (the
/// summit's entry arms read `0x2FB` and `0x281..=0x285`).
const CHAPTER_FLAGS: [u16; 6] = [0x2FB, 0x281, 0x282, 0x283, 0x284, 0x285];
/// The summit's walk-on tile `(57, 76)`, at its centre.
const TRIGGER: (i16, i16) = (57 * 128 + 64, 76 * 128 + 64);

fn page() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).ok()?, String::new())
        .ok()?;
    Some(rt)
}

fn world(rt: &LegaiaRuntime) -> &World {
    &rt.debug_session().expect("session").host.world
}

/// The party a field entry seats: `(leader, present list)`.
fn party(w: &World) -> (u8, Vec<u8>) {
    (w.field_leader(), w.present_party_list())
}

#[test]
fn picker_entry_seats_the_same_lead_on_both_hosts() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    for scene in ["rikuroa", "cave01", "town01"] {
        for flags in [false, true] {
            let mut rt = page().expect("page loads the disc");
            if flags {
                for f in CHAPTER_FLAGS {
                    assert!(rt.set_system_flag(f, true));
                }
            }
            rt.enter_field(scene).expect("page entry");
            for _ in 0..3 {
                rt.set_pad(0);
                rt.tick_frame().expect("tick");
            }
            let page_party = party(world(&rt));

            // The native window's `--scene` entry (`enter_launch_scene`):
            // picker party, the flags ahead of the entry, the picker staging
            // and seat, the live field entry.
            let cfg = legaia_engine_session::session::BootConfig {
                scene: scene.into(),
                enable_audio: false,
            };
            let mut s = legaia_engine_session::native::NativeSession::open_disc_detached_audio(
                std::path::Path::new(&disc),
                &cfg,
            )
            .expect("native session");
            if let Some(d) = s.host.new_game_defaults.as_mut() {
                d.picker_party = true;
            }
            if flags {
                for f in CHAPTER_FLAGS {
                    s.host.world.system_flag_set(f);
                }
            }
            s.host.world.stage_picker_entry(scene, false);
            s.host.arm_picker_entry_seat(scene);
            s.enter_field_live(scene, &Default::default())
                .expect("native entry");
            for _ in 0..3 {
                s.host.world.set_pad(0);
                let _ = s.tick();
            }
            let native_party = party(&s.host.world);
            eprintln!("{scene} flags {flags}: page {page_party:?} native {native_party:?}");
            assert_eq!(
                page_party, native_party,
                "{scene} (chapter flags {flags}): the page seats another lead than the window"
            );
            if scene == "rikuroa" && flags {
                assert_eq!(page_party.0, 1, "Noa leads the summit in her chapter");
            }
        }
    }
}

#[test]
fn the_summit_beat_runs_under_the_crop() {
    let Some(mut rt) = page() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    for f in CHAPTER_FLAGS {
        assert!(rt.set_system_flag(f, true));
    }
    rt.enter_field("rikuroa").expect("enter rikuroa");
    assert!(rt.debug_seat_player(TRIGGER.0, TRIGGER.1));
    // The seat latched the region box at the seated tile.
    let b = world(&rt).terrain.region_attributes.box_bytes;
    assert!(
        (b[0]..b[2]).contains(&57) && (b[1]..b[3]).contains(&76),
        "the seat re-centres the window: box {b:?}"
    );
    let (mut beat, mut cropped) = (0u32, 0u32);
    for t in 0..1200u32 {
        rt.set_pad(if (30..55).contains(&t) { 0x20 } else { 0 });
        rt.tick_frame().expect("tick");
        let w = world(&rt);
        if t > 2 && w.cutscene_timeline_active() {
            beat += 1;
            if field_view_cells(w, CropFraming::Retail).is_some() {
                cropped += 1;
            }
        }
    }
    eprintln!("summit beat: {cropped}/{beat} frames cropped");
    assert!(beat > 600, "the walk-on beat runs ({beat} frames)");
    assert_eq!(
        cropped, beat,
        "every beat frame crops and culls, as retail's does"
    );
}
