//! Disc-gated: King Drake sits on his throne for the audience in `dolk2`.
//!
//! `dolk2` `P2[7]` (the audience after Caruban) fades out, seats the party on
//! the carpet and seats the King on the throne's tile with
//! `CC 46 51 49 78 00 0C` - tile `(73, 120)`, compass index 0, scene clip
//! `12` (the seated pose). The seat calls the floor sampler `FUN_80019278`
//! on him (`jal` at `0x801E1938`, his `+0x10` carries `0x20000`), and the
//! sampler raises the near-bucket bit `0x800000` because the throne's tile is
//! an object-mesh floor without object-grid bit `0x1000`. That bit is what
//! lets him paint over the throne: the seated pose's pelvis stands inside the
//! throne's back, and without it the back buried him for the whole audience.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::scene_actors::bind_scene_actors;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// `P1[41]`, King Drake (target `0x46` less the 29 partition-0 records).
const KING: u8 = 41;
/// The throne's tile centre, `(0x49, 0x78) * 128 + 0x40`.
const THRONE: (i16, i16) = (9408, 15424);

#[test]
fn the_audience_seats_the_king_in_the_near_buckets() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted disc not found (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let cfg = BootConfig {
        scene: "dolk2".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open session");
    s.host.world.system_flag_set(0x142);
    s.enter_field_live("dolk2", &FieldLiveOpts::default())
        .expect("enter dolk2");
    let binding = bind_scene_actors(&mut s.host);
    assert!(s.play_scene_record(2, 7), "dolk2 P2[7] resolves");
    s.set_host_drains_queues(true);

    let mut seated = 0u32;
    for t in 0..4000u32 {
        let pad = if t >= 200 && t % 25 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        let w = &mut s.host.world;
        let _ = w.step_world_frame_tail(
            binding.scene_anm.as_ref(),
            binding.locomotion_anm.as_ref(),
            |slot| binding.clip_targets.get(&slot).copied(),
        );
        if w.npcs.positions.get(&KING) != Some(&THRONE) {
            if seated > 0 {
                break;
            }
            continue;
        }
        assert_eq!(
            w.npcs.clip_cursors.get(&KING).map(|c| c.anim_id),
            Some(12),
            "the seat binds scene clip 12, the seated pose"
        );
        assert!(
            w.field_npc_draw_pose(KING, THRONE).is_some(),
            "the King draws while seated"
        );
        assert!(
            w.near_bucket_npcs().contains(&usize::from(KING)),
            "the seat's floor sample raises 0x800000 on the throne's tile"
        );
        seated += 1;
    }
    assert!(
        seated > 60,
        "the King sits on the throne through the audience ({seated} ticks)"
    );
}
