//! Disc-gated: field-VM op `4C EB` (the "run the next op only if this actor
//! exists" guard) skips **relative** to itself, and the engine host resolves
//! the actor through the scene's channels.
//!
//! koin3's scene-entry script runs a long ladder of `4C EB <id> 05 00` +
//! `B1 <id> 03` pairs between its first BGM start (the town track, global id
//! `2016`) and the one its tile-box test selects (`2055` outside the box, the
//! dance hall's score). While the guard's miss path was read as an
//! **absolute** jump, the first missed lookup sent the script to record byte
//! `5` - inside the record header - where it parked for good, so the scene
//! never got past the town track. A retail save state taken at the dance hall
//! holds `2055` in the BGM word `0x8007BAC8`.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_core::scene::BgmDirector;

#[derive(Default)]
struct Starts(Vec<u16>);

impl BgmDirector for Starts {
    fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
        self.0.push(bgm_id);
    }
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.0.push(bgm_id);
    }
}

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn koin3_entry_script_passes_its_actor_guards_to_the_dance_hall_track() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted disc not found (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let cfg = legaia_engine_shell::boot::BootConfig {
        scene: "koin3".into(),
        enable_audio: false,
    };
    let mut session =
        legaia_engine_shell::boot::BootSession::open(&extracted, &cfg).expect("open koin3");
    let opts = legaia_engine_shell::boot::FieldLiveOpts::default();
    session
        .enter_field_live("koin3", &opts)
        .expect("enter koin3");
    // Stand outside the entry script's tile box `[38,25..55,44]`, where the
    // script takes the dance-hall arm (tile (46, 101)).
    assert!(session.host.world.debug_seat_player(5952, 12992));
    let mut starts = Starts::default();
    for _ in 0..30 {
        session.tick().expect("tick");
        session.host.route_bgm_events(&mut starts).expect("route");
    }
    eprintln!("[koin3] BGM starts: {:?}", starts.0);
    assert_eq!(
        starts.0.first(),
        Some(&2016),
        "the entry script's first start is the town track"
    );
    assert_eq!(
        starts.0.last(),
        Some(&2055),
        "past the 4C EB guards the script selects the dance-hall track"
    );
}
