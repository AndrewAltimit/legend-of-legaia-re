//! Disc-gated executing census of the object and named records that stage
//! an op-`0x45` shot with a release-free path to their end: each runs as a
//! timeline on a live session, then the player walks, and the follow camera
//! must have the frame back.
//!
//! Retail has no latch for a staged shot. Its follow ease `FUN_801DB510`
//! runs from the player's handler (`jal` at `0x801D22B8`) and tests only the
//! camera switch `0x8007B606`, the freeze bit `0x1F800394 & 0x400` and
//! whether the player moved (`0x801DB550..0x801DB5A4`), so once a record
//! lets the player go, the first walk eases off whatever shot it left. The
//! port's `script_owns_focus` latch outlived the record and froze the
//! follow camera on the shot for the rest of the scene
//! (`Camera::release_unheld_shot` is the fix).
//!
//! The list is every partition-0 / partition-2 row of
//! `engine-core/tests/field_camera_shot_census_disc.rs` with a CONFIGURE
//! that reaches its record's end without an APPLY, a `[4C 39]` / `[4C 3E]`
//! snap or a scene change (`other7`, a debug scene, left out). Records are
//! installed without their story gates; one that waits on something the
//! probe does not provide, leaves the scene, or hands the walk to another
//! record is reported and not judged.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_scene::scene::{ProtIndex, Scene};
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

/// `(scene, partition, record)`.
const RECORDS: &[(&str, usize, usize)] = &[
    ("balden", 0, 7),
    ("balden", 0, 14),
    ("balden2", 0, 7),
    ("balden2", 0, 14),
    ("cave01", 2, 9),
    ("cave01", 2, 10),
    ("cave01", 2, 11),
    ("chitei2", 2, 15),
    ("conc2", 2, 11),
    ("conc2", 2, 13),
    ("concnow", 2, 17),
    ("deroa", 2, 10),
    ("dohaty", 2, 12),
    ("doman", 2, 0),
    ("doman", 2, 2),
    ("doman", 2, 7),
    ("dream", 2, 1),
    ("dream", 2, 2),
    ("dream", 2, 3),
    ("dream", 2, 4),
    ("garmel", 2, 14),
    ("jagaroom", 2, 7),
    ("jiji", 2, 10),
    ("jou", 2, 4),
    ("jouina", 2, 22),
    ("jouinb", 2, 8),
    ("jouine", 2, 16),
    ("koin3", 2, 3),
    ("map03", 2, 14),
    ("nilboa", 2, 1),
    ("nilboa", 2, 2),
    ("nilboa", 2, 3),
    ("nilboa", 2, 10),
    ("nilboa", 2, 24),
    ("nilboa", 2, 28),
    ("rayman", 2, 16),
    ("rayman", 2, 19),
    ("rayman2", 2, 16),
    ("retona", 2, 23),
    ("retona", 2, 24),
    ("stone", 2, 5),
    ("town01", 2, 6),
    ("town01", 2, 25),
    ("town0b", 2, 9),
    ("town0b", 2, 23),
    ("town0b", 2, 30),
    ("town0c", 2, 29),
    ("town0d", 2, 26),
    ("town0d", 2, 27),
    ("town0d", 2, 29),
    ("town0d", 2, 30),
    ("town0d", 2, 31),
    ("town0e", 2, 5),
    ("tunnelc", 2, 5),
    ("uru", 2, 30),
    ("uru", 2, 32),
    ("uru", 2, 34),
    ("uru", 2, 37),
    ("vozz", 2, 17),
];

fn holds(s: &NativeSession) -> bool {
    let w = &s.host.world;
    w.cutscene_timeline_active() || w.dialogue_owns_input() || w.script_context_engages_player()
}

#[test]
fn unreleased_record_shots_return_to_the_follow_camera() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open PROT index");
    let mut judged = 0;
    let mut frozen = Vec::new();
    for &(scene, partition, record) in RECORDS {
        let Ok(sc) = Scene::load(&index, scene) else {
            continue;
        };
        let Ok(Some(man)) = sc.field_man_payload(&index) else {
            continue;
        };
        let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        let cfg = BootConfig {
            scene: scene.into(),
            enable_audio: false,
        };
        let Ok(mut s) = NativeSession::open(&extracted, &cfg) else {
            continue;
        };
        s.host.world.toggles.use_vm_dialogue = true;
        if s.enter_field_live(scene, &FieldLiveOpts::default())
            .is_err()
        {
            continue;
        }
        s.set_host_drains_queues(true);
        // Let the scene's own entry beats run out first.
        for t in 0..3000u32 {
            if t >= 30 && !holds(&s) {
                break;
            }
            s.host.world.set_pad(if t % 8 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            });
            s.tick().expect("tick");
        }
        let label = s.host.world.active_scene_label.clone();
        if !s
            .host
            .world
            .install_cutscene_timeline_record(&mf, &man, partition, record, false)
        {
            continue;
        }
        let (mut staged, mut ended) = (false, false);
        for t in 0..6000u32 {
            if s.host.world.active_scene_label != label
                || s.host.world.scene_transition_hold.is_some()
            {
                break;
            }
            staged |= s.camera.script_owns_focus();
            if !holds(&s) {
                ended = true;
                break;
            }
            s.host.world.set_pad(if t % 8 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            });
            s.tick().expect("tick");
        }
        let mut retriggered = false;
        for _ in 0..90 {
            s.host.world.set_pad(PadButton::Down.mask());
            s.tick().expect("tick");
            retriggered |= holds(&s);
        }
        s.host.world.set_pad(0);
        let latched = s.camera.script_owns_focus();
        eprintln!(
            "RECORD {scene} P{partition}[{record}] staged={staged} ended={ended} \
             retriggered={retriggered} latched_after_walk={latched}"
        );
        if staged && ended && !retriggered {
            judged += 1;
            if latched {
                frozen.push((scene, partition, record));
            }
        }
    }
    eprintln!("[records] {} listed, {judged} judged", RECORDS.len());
    assert!(
        judged >= 20,
        "only {judged} records staged a shot and ended with the player free"
    );
    assert!(
        frozen.is_empty(),
        "the follow camera stayed on a released record's shot: {frozen:?}"
    );
}
