//! Disc-gated: per-actor field-VM channels are seeded and executed on an
//! **ordinary free-roam** scene load - not just under a cutscene timeline.
//!
//! Cold-boots `town01` (the Rim Elm free-roam scene the prologue hands off to)
//! live through `SceneHost` and asserts:
//!
//! 1. entering it does NOT install a cutscene timeline (this is the free-roam
//!    path, the half of retail's `FUN_8003AEB0` spawn loop that runs with no
//!    timeline driving cross-context pokes);
//! 2. one channel is seeded per partition-1 placement record, with the retail
//!    script-id rule `partition-0 count + record index` (`FUN_8003A1E4`) - on
//!    the pre-change engine this set was empty outside a cutscene, so a
//!    non-empty set is the regression-proof of the new seeding;
//! 3. ticking the world in Field mode steps the channels: at least one runs its
//!    own init opcodes past the entry PC and a channel context's state changes
//!    (scripted facing / `WAIT` cadence / local-flag setup lands);
//! 4. a scripted move surfaces a heading, so a repositioned NPC no longer
//!    renders unrotated (reported; the exact set is scene-script dependent).
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
use std::path::PathBuf;

/// Prefer an already-extracted tree; fall back to opening the disc `.bin`
/// directly (no extraction step needed).
fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

#[test]
fn freeroam_channels_seed_and_execute() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };

    let scene = legaia_asset::new_game::OPENING_SCENE; // town01, free-roam
    host.enter_field_scene(scene, 0).expect("enter town01");

    // 1. Free-roam path: no cutscene timeline drives this scene.
    assert!(
        !host.world.cutscene_timeline_active(),
        "town01 is a free-roam scene - no cutscene timeline is installed"
    );

    // 2. Channels seeded from the placement partition (the new behaviour; empty
    //    outside a cutscene before this change).
    let n = host.world.field_channels.len();
    assert!(
        n > 0,
        "free-roam scene entry seeds one channel per partition-1 placement"
    );
    assert!(
        host.world.field_channels_man.is_some(),
        "the seeded channels carry their MAN buffer so they can step"
    );
    for c in &host.world.field_channels {
        assert!(
            c.ctx.script_id > 0,
            "script id = partition-0 count + record index (>= 1 for a real placement)"
        );
    }
    let spawn_state: Vec<_> = host
        .world
        .field_channels
        .iter()
        .map(|c| {
            (
                c.pc,
                c.ctx.flags,
                c.ctx.local_flags,
                c.ctx.face_rotation,
                c.ctx.wait_accum,
                c.ctx.world_x,
                c.ctx.world_z,
            )
        })
        .collect();
    eprintln!(
        "[town01] {n} free-roam channels seeded, script ids {:?}",
        host.world
            .field_channels
            .iter()
            .map(|c| c.ctx.script_id)
            .collect::<Vec<_>>()
    );

    // 3. Tick in Field mode and observe channel execution.
    let headings_before = host.world.field_npc_headings.len();
    let mut any_advanced = false;
    let mut any_state_changed = false;
    for _ in 0..600 {
        let _ = host.world.tick();
        for (c, s) in host.world.field_channels.iter().zip(&spawn_state) {
            if c.pc != s.0 {
                any_advanced = true;
            }
            if (
                c.ctx.flags,
                c.ctx.local_flags,
                c.ctx.face_rotation,
                c.ctx.wait_accum,
                c.ctx.world_x,
                c.ctx.world_z,
            ) != (s.1, s.2, s.3, s.4, s.5, s.6)
            {
                any_state_changed = true;
            }
        }
    }

    assert!(
        any_advanced,
        "at least one free-roam channel executed past its entry PC (init opcodes run)"
    );
    assert!(
        any_state_changed,
        "at least one channel's context state changed (facing / wait / flag setup applied)"
    );
    let headings_after = host.world.field_npc_headings.len();
    eprintln!(
        "[town01] channels advanced; NPC headings before={headings_before} after={headings_after}"
    );
}
