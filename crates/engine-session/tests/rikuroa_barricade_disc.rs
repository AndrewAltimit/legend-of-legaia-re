//! `rikuroa`'s barricade (MAN partition-0 record 1, the object on tile
//! `(17, 41)`): its spawn prologue tests `0x142` (Caruban defeated) and only
//! the set arm carries the `31 00` that makes it collision-exempt. With the
//! flag clear it stands closed and solid, and walking into it runs its
//! record's touch body, which plays the opening clip and raises `31 00`.
//! The class bits used to be read off a linear walk of both arms, so the
//! barricade spawned exempt but drawn closed: the player walked through a
//! shut gate and it never swung open (issue #582's last item).
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

const BARRICADE: (u8, u8) = (17, 41);

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn barricade(s: &NativeSession) -> (bool, bool) {
    let w = &s.host.world;
    let prop = w
        .props
        .bank
        .props
        .get(&BARRICADE)
        .expect("the barricade is a posed prop");
    let solid = w
        .props
        .colliders
        .iter()
        .find(|c| c.anchor == Some(BARRICADE))
        .expect("the barricade has a collider row")
        .solid;
    (solid, prop.collision_exempt())
}

#[test]
fn the_barricade_stands_shut_until_the_player_walks_into_it() {
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
    s.resume_save(save, "rikuroa", &FieldLiveOpts::default());
    assert!(
        !s.host.world.system_flag_test(0x142),
        "Caruban not yet beaten"
    );
    // The summit-climb story state the scene's entry settles under (the
    // same seat `rikuroa_summit_cutscene_cull_disc` takes); `0x142` stays
    // clear.
    for f in [0x2FB, 0x281, 0x282, 0x283, 0x284, 0x285] {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..120 {
        s.tick().expect("tick");
    }

    assert_eq!(
        barricade(&s),
        (true, false),
        "with 0x142 clear the barricade spawns solid, not exempt"
    );

    // Stand on the path below it and walk up into it.
    assert!(s.host.world.debug_seat_player(2240, 5056), "the path tile");
    s.host.world.refresh_region_attributes_at_player();
    let mut opened_at = None;
    for t in 0..600 {
        s.host
            .world
            .set_pad(if t < 120 { PadButton::Up.mask() } else { 0 });
        s.tick().expect("tick");
        if opened_at.is_none() && barricade(&s).1 {
            opened_at = Some((t, s.host.world.actors[0].move_state.world_z));
        }
    }
    let (_, z) = opened_at.expect("the touch runs the barricade's body, which raises `31 00`");
    assert!(
        i32::from(z) < 5312 - 64,
        "the shut barricade stopped the walk short of its tile (z {z})"
    );
}
