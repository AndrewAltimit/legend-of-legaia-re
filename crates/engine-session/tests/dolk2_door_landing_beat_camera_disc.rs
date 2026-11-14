//! A walk-on beat spawned by a door's landing composes its hand-back from
//! the landing's camera block, not the block of the room the player left.
//!
//! `dolk2`'s door out of the castle hall lands on row `94` above the market
//! stairs, which is the walk-on row of Noa's market beat (`P2[11]`, tiles
//! `(69..=71, 94)`). Retail's landing (`FUN_801D1EC4`) runs the zone query
//! `FUN_801DE3E0` and the snap `FUN_801DB8EC` at `0x801D1FF4..0x801D2010`,
//! before `FUN_8003BDE0` spawns the landing tile's record at `0x801D2058`.
//! The port queued that query for the follow tick, which does not run while
//! the beat holds the shot, so the beat's closing `45 C0 78 00` composed the
//! hall's follow shot (pitch `494`, yaw `-630`, `H` `768`) and glided the
//! camera round Vahn to it before the hand-back snapped to the market's.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

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

/// The market's follow pose at the stair head: pitch, yaw, `H`.
const MARKET_FOLLOW: (i32, i32, i32) = (200, 0, 520);

#[test]
fn the_market_beat_hands_back_to_the_market_shot() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "dolk2".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.toggles.use_vm_dialogue = true;
    // After Caruban and the King's audience (`P2[7]` raises these), before
    // the market beat (`0x27C` / `0x27D` still clear).
    for f in [0x142u16, 0x178, 0x179, 0x26A] {
        s.host.world.system_flag_set(f);
    }
    s.enter_field_live("dolk2", &FieldLiveOpts::default())
        .expect("enter dolk2");
    s.set_host_drains_queues(true);

    let mut beat_seen = false;
    let mut hand_back = None;
    for t in 0..2400u32 {
        // The door tile into the hall; the held Down walks into the hall and
        // straight back out through its door onto the beat's row.
        if t == 5 {
            s.host.world.seat_player_at_tile(70, 96);
        }
        let held = s.host.world.cutscene_timeline_active();
        beat_seen |= held;
        let pad = if !held && !beat_seen && t > 60 {
            PadButton::Down.mask()
        } else if s.host.world.dialogue_owns_input() && t % 6 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        // The closing APPLY's glide is the record's only 120-frame one.
        if let Some(mv) = s.camera.mover.as_ref()
            && mv.d == 120
        {
            hand_back = Some((mv.pairs[0].1, mv.pairs[1].1, mv.pairs[9].1));
            break;
        }
    }
    assert!(beat_seen, "the landing spawns the market beat");
    let end = hand_back.expect("the beat reaches its closing APPLY");
    assert_eq!(
        end, MARKET_FOLLOW,
        "the hand-back glides to the market's shot"
    );
}
