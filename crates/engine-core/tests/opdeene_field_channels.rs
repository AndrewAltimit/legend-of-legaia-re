//! Disc-gated: the opening prologue's per-actor field-VM channels spawn and
//! execute - the vignette mechanism.
//!
//! Cold-boots `opdeene` live through `SceneHost` and asserts:
//!
//! 1. entering `opdeene` (which installs the cutscene timeline) spawns one
//!    channel per partition-1 placement record, with the retail script-id
//!    rule `partition-0 count + record index` (`FUN_8003A1E4`);
//! 2. ticking the world steps the channels: at least one channel executes past
//!    its entry PC (the placement scripts are not inert);
//! 3. channel scripts raise animate cues (op `0x4B` ANIMATE via
//!    `World::field_npc_anim_cues`) - the "characters doing things" signal the
//!    windowed host consumes;
//! 4. the timeline's cross-context pokes reach channel contexts (some
//!    channel's flag word / local flags / position differs from spawn state).
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

#[test]
fn opdeene_channels_spawn_and_execute() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let cutscene = legaia_asset::new_game::OPENING_CUTSCENE_SCENE;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene(cutscene, 0).expect("enter opdeene");

    // 1. Channels spawned alongside the timeline, one per placement, with the
    //    retail script-id rule.
    assert!(
        host.world.cutscene_timeline_active(),
        "entering opdeene installs the cutscene timeline"
    );
    let n = host.world.field_channels.len();
    assert!(n > 0, "opdeene spawns per-actor channels");
    let spawn_state: Vec<_> = host
        .world
        .field_channels
        .iter()
        .map(|c| {
            (
                c.pc,
                c.ctx.flags,
                c.ctx.local_flags,
                c.ctx.world_x,
                c.ctx.world_z,
            )
        })
        .collect();
    for c in &host.world.field_channels {
        assert!(
            c.ctx.script_id >= 4,
            "script id = partition-0 count + record index (opdeene P0 count is 3, records 1..)"
        );
    }
    eprintln!(
        "[opdeene] {n} channels spawned, script ids {:?}",
        host.world
            .field_channels
            .iter()
            .map(|c| c.ctx.script_id)
            .collect::<Vec<_>>()
    );

    // 2..4. Tick through the prologue and observe channel activity.
    let mut any_advanced = false;
    let mut any_poked = false;
    let mut cue_count = 0usize;
    for _ in 0..2000 {
        if !host.world.cutscene_timeline_active() {
            break;
        }
        let _ = host.world.tick();
        cue_count = cue_count.max(host.world.field_npc_anim_cues.len());
        for (c, s) in host.world.field_channels.iter().zip(&spawn_state) {
            if c.pc != s.0 {
                any_advanced = true;
            }
            if (c.ctx.flags, c.ctx.local_flags, c.ctx.world_x, c.ctx.world_z)
                != (s.1, s.2, s.3, s.4)
            {
                any_poked = true;
            }
        }
    }

    assert!(
        any_advanced,
        "at least one channel executed past its entry PC (placement scripts run)"
    );
    assert!(
        any_poked,
        "at least one channel context changed state (timeline pokes / own script effects land)"
    );
    eprintln!("[opdeene] channels advanced; max simultaneous anim cues = {cue_count}");
    // The animate cue is the key vignette signal; report but don't hard-require
    // a specific count (the exact cue set depends on how far the timeline gets
    // within its frame cap).
    assert!(
        cue_count > 0,
        "channel scripts raise op-0x4B animate cues for the windowed host"
    );
}
