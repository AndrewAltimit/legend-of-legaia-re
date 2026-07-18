//! Disc-gated: the New-Game opening fades in from black, driven by real disc
//! bytecode.
//!
//! Retail opens the prologue with a fade-from-black (cold-boot capture: the
//! screen ramps up from black before the creation crawl begins). The fade is
//! authored in every field scene's `P1[0]` entry script as the arrival arm of
//! the `0x52F`/`0x530`/`0x531` scene-transition fade handshake:
//! `4C 12 00 00 00 00 00` (global multiply tint -> black, instant) then
//! `4C 12 80 80 80 44 00` (ramp to neutral `0x80` over 68 frames). New Game
//! arms the handshake (`begin_new_game` sets sysflag `0x52F`, the boot-side
//! stage), and `opdeene`'s entry script fires the arm through the field VM's
//! `menu_ctrl_sub1` host hook into `World::screen_tint`.
//!
//! Also pins the opening timeline's op-`0x34` sub-0 effect-layer colour ramps
//! (e.g. `34 05 00 00 00 D2 00` = ramp to black over 210 frames) firing into
//! `World::effect_tint` during the crawl. NB that value is NOT a screen fade
//! (the retail capture holds the lit tableau across its black spans); the
//! test pins the ramp value model only.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
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

fn skip_or_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

#[test]
fn new_game_opdeene_entry_fades_in_from_black() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    host.world.begin_new_game();
    assert!(
        host.world.system_flag_test(0x52F),
        "New Game arms the arrival fade handshake (sysflag 0x52F)"
    );
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");

    // The entry script's arrival arm fires within the load-frame pre-run:
    // the 4C-12 screen-tint channel opens at (or near) black on the very
    // first tick. Track `World::screen_tint` (the 4C-12 channel) directly -
    // the combined `scene_screen_tint` also carries the timeline's op-0x34
    // between-beat fade, which overlaps mid-ramp.
    let _ = host.tick();
    let t0 = host
        .world
        .screen_tint
        .as_ref()
        .map(|t| t.factor())
        .expect("opdeene entry fires the 4C 12 fade arm on the load frame");
    assert!(
        t0[0] < 0.10 && t0[1] < 0.10 && t0[2] < 0.10,
        "the screen-tint channel opens at black (got {t0:?})"
    );
    eprintln!("[fade] screen tint {t0:?} at tick 0");

    // The authored ramp (`4C 12 80 80 80 44 00`) then lifts the tint back to
    // neutral over 68 frames: require a monotonic rise that completes (the
    // channel drops = identity) within a small margin of the ramp.
    let mut mid_seen = false;
    let mut cleared_after = None;
    let mut prev = t0[0];
    for tick in 1..120u32 {
        let _ = host.tick();
        match host.world.screen_tint.as_ref().map(|t| t.factor()) {
            Some(t) => {
                assert!(
                    t[0] >= prev - 1e-3,
                    "fade-in must rise monotonically (tick {tick}: {} < {prev})",
                    t[0]
                );
                prev = t[0];
                if t[0] > 0.4 && t[0] < 0.9 {
                    mid_seen = true;
                }
            }
            None => {
                cleared_after = Some(tick);
                break;
            }
        }
    }
    assert!(
        mid_seen,
        "the ramp passes through mid-grey (a real fade, not a cut)"
    );
    let cleared_after = cleared_after.expect("the fade lands on neutral and clears");
    assert!(
        (60..=90).contains(&cleared_after),
        "the fade-in spans the authored 68-frame ramp (cleared after {cleared_after} ticks)"
    );
    eprintln!("[fade] tint returned to identity after {cleared_after} ticks");
}

#[test]
fn opdeene_timeline_fires_between_beat_black_fades() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    host.world.begin_new_game();
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");
    assert!(host.world.cutscene_timeline_active());

    // Drive the opening timeline and watch the op-0x34 effect-layer colour:
    // the between-block gap authors `34 05 00 00 00 D2 00` (ramp to black
    // over 210 frames) followed by `34 01 FF FF FF 00 00` (instant neutral),
    // so during the crawl the value must both drop below half and later
    // return to identity. (A value model only - not a screen fade.)
    let mut saw_dark = false;
    let mut saw_recover = false;
    for _ in 0..4000u32 {
        let _ = host.tick();
        match host.world.effect_tint.as_ref().map(|t| t.factor()) {
            Some(t) if t[0] < 0.5 => saw_dark = true,
            _ => {
                if saw_dark {
                    saw_recover = true;
                }
            }
        }
        if saw_dark && saw_recover {
            break;
        }
        if host.world.active_scene_label != opdeene {
            break;
        }
    }
    assert!(
        saw_dark,
        "the opening timeline's op-0x34 black fade darkens the effect tint below half"
    );
    assert!(
        saw_recover,
        "the effect tint recovers to identity after the between-beat black fade"
    );
}
