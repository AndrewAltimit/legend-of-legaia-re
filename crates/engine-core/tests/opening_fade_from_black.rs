//! `World::screen_tint_pushes` during the crawl. NB that value is NOT a screen fade
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
    // first tick. Track `World::presentation.tint` (the 4C-12 channel) directly -
    // the combined `scene_screen_tint` also carries the timeline's op-0x34
    // between-beat fade, which overlaps mid-ramp.
    let _ = host.tick();
    let t0 = host
        .world
        .presentation
        .tint
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
        match host.world.presentation.tint.as_ref().map(|t| t.factor()) {
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
        // The op-0x34 effect is a pool colour tween now, so the observable
        // is the frame's `FUN_80024EE4` push rather than a float factor.
        // "Dark" is a push whose red lane sits below half.
        match host
            .world
            .screen_tint_pushes()
            .iter()
            .map(|p| (p.packed & 0xFF) as u8)
            .max()
        {
            Some(r) if r < 0x80 => saw_dark = true,
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
        "the opening timeline's op-0x34 walk-out pushes a colour below half"
    );
    assert!(
        saw_recover,
        "the effect tint recovers to identity after the between-beat black fade"
    );
}

/// The entry script's load-frame run ends where retail's system context
/// stands once the opening record has taken the player.
///
/// Retail runs the install slice to the first executed `0x21`
/// (`FUN_8003AB2C`), then one `0x21`-bounded pass per frame until the record
/// the body spawns (`44 23`) holds the player. The `s1_newgame_field` capture
/// (a cold New Game in `opdeene`) holds the system context at PC `0x99` - just
/// past the spawning pass's `0x21` - with one bit of the region selector
/// `0x19B..0x1AA` up, the one the body set at the arrival tile. The engine
/// installs the opening as its timeline at scene entry, so it runs those
/// passes in the load frame; running on through the per-frame loop instead
/// left the pass open mid-selector, and the next tick re-evaluated it
/// wherever the player then stood.
///
/// Which bit is not pinned here: retail's is `0x1A2` (region type 7, tiles
/// `x 8..=35, z 89..=115`), while the engine stands the New Game player on
/// tile `(56, 19)` (type 3, `0x19E`) before the opening moves him.
#[test]
fn new_game_opdeene_entry_run_stops_where_retail_parks() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    let opdeene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    host.world.begin_new_game();
    host.enter_field_scene(opdeene, 0).expect("enter opdeene");
    assert_eq!(host.world.field_pc, 0x99, "system PC after the load frame");
    assert!(!host.world.field_vm.system_pass_open, "the pass is closed");
    assert!(
        host.world.system_flag_test(0x566),
        "the opening spawn latch"
    );
    let band: Vec<u16> = (0x19B..=0x1AA)
        .filter(|&f| host.world.system_flag_test(f))
        .collect();
    assert_eq!(band.len(), 1, "one region-selector bit: {band:x?}");
    // The opening holds the player from here: the system loop sits out and
    // the selector stays put.
    for _ in 0..30 {
        let _ = host.tick();
    }
    assert_eq!(host.world.field_pc, 0x99);
    let after: Vec<u16> = (0x19B..=0x1AA)
        .filter(|&f| host.world.system_flag_test(f))
        .collect();
    assert_eq!(after, band, "the selector holds while the opening runs");
}
