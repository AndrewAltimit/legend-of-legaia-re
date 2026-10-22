//! Disc-gated: where `kor5`'s Old Deez (`P1[7]`, flat index 14) stands, on
//! a fresh entry and after the Genesis Tree cutscene.
//!
//! - **Fresh entry.** `FUN_8003A1E4` seats a placement from its 4-byte
//!   header; Z byte `0xDC` is tile 92 with bit 7 set, so `92 * 128 + 0x80`
//!   (`0x8003A370..0x8003A384`) = `11904`. The spawn section (`25`,
//!   `38 80 00`, `21`) only sets a facing. `kor5_field_card_boot` holds him
//!   there.
//! - **After the cutscene.** `P2[3]` walks him (cross-context target
//!   `0x0E`) and near its end seats him with `A3 0E 20 5D`: tile `(32, 93)`,
//!   `(4160, 11968)`. It then sets system flag `0x43A`. The
//!   `kor5_post_436_organic` / `kor5_post_43a_checkpoint` captures were
//!   taken in that same visit and hold him there.
//!
//! Nothing else on the disc moves `0x0E`, and the spawn section has no flag
//! branch. A later entry seats him back on the header tile, so the
//! retail-compare seed (a fresh entry with the capture's flags) shows the
//! header seat against those two captures. That is a harness artefact; this
//! test pins that the engine itself reaches both seats.
//!
//! Skips + passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Old Deez's partition-1 slot.
const DEEZ: u8 = 7;

#[test]
fn kor5_old_deez_takes_the_header_seat_then_the_cutscene_seat() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("kor5", 0).expect("enter kor5");
    eprintln!("[ran] kor5");
    assert_eq!(
        host.world.npcs.positions.get(&DEEZ).copied(),
        Some((4160, 11904)),
        "a fresh entry seats P1[7] on its header tile (Z byte 0xDC: row 92 + 0x80)"
    );
    assert!(!host.world.system_flag_test(0x43A));

    let man = host
        .scene
        .as_ref()
        .expect("scene")
        .field_man_payload(&host.index)
        .expect("read MAN")
        .expect("kor5 carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man, 2, 3, false),
        "kor5 P2[3] installs"
    );
    // The cutscene's text boxes advance on Cross.
    let cross = legaia_engine_system::input::PadButton::Cross.mask();
    let mut done = false;
    for t in 0..12_000u32 {
        host.world
            .input
            .set_pad(if t.is_multiple_of(2) { cross } else { 0 });
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
        if host
            .world
            .cutscene
            .timeline
            .as_ref()
            .is_none_or(|tl| tl.done)
        {
            done = true;
            break;
        }
    }
    assert!(done, "kor5 P2[3] runs to its end");
    assert_eq!(
        host.scene.as_ref().map(|s| s.name.as_str()),
        Some("kor5"),
        "the cutscene ends in kor5"
    );
    assert!(
        host.world.system_flag_test(0x43A),
        "P2[3] sets system flag 0x43A"
    );
    assert_eq!(
        host.world.npcs.positions.get(&DEEZ).copied(),
        Some((4160, 11968)),
        "P2[3]'s `A3 0E 20 5D` seats Old Deez where the post-cutscene captures hold him"
    );
}
