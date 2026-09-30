//! Pins the native half of the shared resume / New Game entry
//! (`BootSession::resume_save` / `BootSession::start_new_game`, both over
//! `legaia_engine_core::resume`). The browser page's `play_resume_save` /
//! `play_new_game` pin the same landings in `crates/web-viewer/src/resume.rs`.
//!
//! Skip-passes without disc data / extracted assets (the `LEGAIA_DISC_BIN`
//! convention).

use std::path::PathBuf;

use legaia_engine_core::resume::ResumeLanding;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open() -> Option<BootSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    Some(BootSession::open(&extracted, &cfg).expect("open boot session"))
}

#[test]
fn resume_lands_in_the_saved_scene_else_over_the_running_one_never_a_new_game() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");

    session.host.world.party.money = 4321;
    let save = session.host.world.save_full();

    // A save naming a scene this host cannot enter loads over the running
    // scene - it neither re-enters anything nor becomes a New Game.
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "no_such_scene", &opts);
    assert_eq!(landing, ResumeLanding::CurrentScene("town01".into()));
    assert_eq!(session.host.world.party.money, 4321, "the save applied");
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town01")
    );

    // The saved scene is entered, even when it is the one already running.
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "town01", &opts);
    assert_eq!(landing, ResumeLanding::SavedScene("town01".into()));
    assert_eq!(session.host.world.party.money, 4321);
    session.host.world.party.money = 7;
    let landing = session.resume_save(save.clone(), "town0b", &opts);
    assert_eq!(landing, ResumeLanding::SavedScene("town0b".into()));
    assert!(landing.entered_scene());
    assert_eq!(
        session.host.scene.as_ref().map(|s| s.name.as_str()),
        Some("town0b")
    );
    assert_eq!(
        session.host.world.party.money, 4321,
        "the save lands after the entry"
    );
    eprintln!("[ok] resume_save: unenterable -> current, named -> saved scene");
}

#[test]
fn start_new_game_seeds_and_enters_the_prologue() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    session
        .enter_field_live("town01", &opts)
        .expect("enter town01");
    session.host.world.party.money = 4321;
    let entered = session.start_new_game(&opts);
    assert_eq!(
        entered,
        Some(legaia_asset::new_game::OPENING_CUTSCENE_SCENE),
        "a New Game enters the prologue cutscene scene"
    );
    assert_eq!(
        session.host.world.party.money,
        legaia_engine_core::world::NEW_GAME_STARTING_GOLD
    );
    if session.starting_party.is_some() {
        assert_eq!(session.host.world.party.party_count, 1, "Vahn alone");
    }
    eprintln!("[ok] start_new_game entered {entered:?}");
}

/// A card load of a four-member party onto a field raises no actor but the
/// player's, and the frame it lands on keeps its ground.
///
/// `World::load_party` used to raise actor slot `n` for every roster record
/// `n`. On a field those slots are the scene's: the idle ones carry the
/// scene-pack meshes `init_scene_animations` pre-binds, so a resume of a
/// four-member save drew slots 1..3 at the origin - on `uru` its sky and
/// cliff pack, a wall across the right half of the frame that hid the
/// ground at the `uru_field_run` seat (world `(4864, 880)`, tile `(38, 6)`).
/// The visible-tile crop at the same seat is checked alongside: every ground
/// cell of the retail view around the player is inside the rectangle the
/// field render library walks.
#[test]
fn a_four_member_resume_on_uru_raises_only_the_player_and_keeps_the_ground() {
    let Some(mut session) = open() else { return };
    let opts = FieldLiveOpts::default();
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(4),
        ..Default::default()
    };
    let landing = session.resume_save(save, "uru", &opts);
    assert_eq!(landing, ResumeLanding::SavedScene("uru".into()));
    let (x, z) = (4864i16, 880i16);
    assert!(
        session.host.world.debug_seat_player(x, z),
        "seat the player"
    );
    session.camera.zone.arm_arrival();
    for _ in 0..30 {
        session.tick().expect("tick");
    }
    let world = &session.host.world;
    assert_eq!(world.party.roster.members.len(), 4);
    let player = usize::from(world.player_actor_slot.expect("a field player"));
    // Non-vacuous: the scene pre-binds meshes onto the idle party-index
    // slots, which is what made a raise there visible.
    assert!(
        (1..4).any(|s| world.actors[s].tmd_binding.is_some()),
        "uru pre-binds scene-pack meshes onto slots 1..3"
    );
    for slot in 0..4 {
        assert_eq!(
            world.actor_slot_drawn(slot, false),
            slot == player,
            "actor {slot} drawn after the resume"
        );
    }

    // The crop at the seat keeps the ground the retail frame shows: the
    // player's column band and the rows from just behind the player to well
    // past the top of the frame.
    let cells = legaia_engine_core::field_view_window::field_view_cells(world, true)
        .expect("the crop is live at the retail seat");
    let (tx, tz) = (i32::from(x) >> 7, i32::from(z) >> 7);
    for col in tx - 8..=tx + 8 {
        for row in tz - 2..=tz + 12 {
            assert!(
                cells.ground_visible(col, row),
                "ground cell ({col}, {row}) cropped at the uru_field_run seat: {cells:?}"
            );
        }
    }
    eprintln!("[ran] uru four-member resume: player-only actors, crop {cells:?}");
}
