//! `vell`'s Bridge Grass (MAN partition-0 record 1, the object on tile
//! `(16, 37)`): with the Fertilizer offered (`0x078`) and the first-visit beat
//! seen (`0x460`), examining it asks "Do you wish to use the Fertilizer?".
//! Yes sets `0x2AE`, takes the item, re-arms the object's VDF morph
//! (`4B 07 00 ..`), drops the pulse's recycle and hold bits (`AC 01 0C`,
//! `AC 01 0A`), primes the envelope (`AB 01 0D`) and spins on its ADVANCE
//! latch (`AC 01 0B` / `AD 01 0B`) while the grass grows across the river.
//!
//! The spin reads the record's own context and the envelope ran on the
//! object's channel, so the latch never reached the spin: the examine parked
//! on `AD 01 0B` for good and the player stood frozen at the riverbank.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

const GRASS: (u8, u8) = (16, 37);

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
fn the_fertilizer_grows_the_grass_and_hands_the_player_back() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, "vell", &FieldLiveOpts::default());
    for f in [0x460, 0x078] {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..60 {
        s.tick().expect("tick");
    }
    assert!(
        s.host.world.props.bank.props.contains_key(&GRASS),
        "the Bridge Grass is a banked prop"
    );
    assert!(
        s.host.world.start_prop_interaction(GRASS),
        "the grass's record starts"
    );
    // Page the text and take the picker's first row (Yes).
    let mut used_at = None;
    let mut done_at = None;
    for t in 0..3000u32 {
        let pad = if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        if used_at.is_none() && s.host.world.system_flag_test(0x2AE) {
            used_at = Some(t);
        }
        if used_at.is_some() && s.host.world.dialog.inline.is_none() {
            done_at = Some(t);
            break;
        }
    }
    let used = used_at.expect("Yes on the Fertilizer picker sets 0x2AE");
    let done = done_at.unwrap_or_else(|| {
        let pc = s.host.world.dialog.inline.as_ref().map(|id| id.pc);
        panic!("the grass record never handed the player back (parked at {pc:x?})")
    });
    // The lanes unwind over a few hundred frames: the hand-back waits on
    // them rather than stepping past the spin.
    assert!(
        done > used + 60,
        "the record waited on the grass's envelope (used {used}, done {done})"
    );
}
