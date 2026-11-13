//! Disc-gated: the overworld pan after the Genesis Tree revives hides Vahn.
//!
//! `rikuroa` `P2[53]` changes scene to `map01` with `0x2C2` up, and the
//! world map's system script spawns `P2[29]`: a pan from the tree's light to
//! Drake Castle, then the hop into `dolk`. Its first seat is
//! `CC F8 51 7F 7F 80 02`, the parked-sentinel tile on the player. The
//! `4C 51` arm stores the tile into `+0x14` / `+0x18` before the identity
//! compare that splits the player arm off (`0x801E1880` / `0x801E1898`, then
//! `0x801E1954`), so Vahn stands in the hide box for the whole pan; the port
//! used to skip the seat on the player and drew him on his arrival tile
//! outside the castle.
//!
//! Seeded from the `PRO-00` card save with the flags the beats before it
//! raise. Skip-passes without `LEGAIA_DISC_BIN`, an extracted tree or the
//! save library.

use std::path::{Path, PathBuf};

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn card_sc(lib: &Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&lib.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

/// `(0x7F * 128 + 0x40)`: the parked-sentinel tile's world coordinate.
const HIDE_XZ: i16 = 0x3FC0;

#[test]
fn the_tree_pan_parks_vahn_off_the_map() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-00") else {
        eprintln!("[skip] save library card PRO-00 missing");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let mut save =
        legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
            .expect("lift SC block");
    for f in [0x142u16, 0x28A, 0x2C2] {
        save.ext.story_flag_bits[0x158 + (f as usize >> 3)] |= 0x80 >> (f & 7);
    }
    s.host.world.load_full(save.clone());
    let landing = s.resume_save(save, "map01", &FieldLiveOpts::default());
    assert!(landing.entered_scene(), "resume onto map01: {landing:?}");

    // Frames the pan record runs with the camera configured past its seat,
    // and where Vahn stood on each.
    let mut pan_frames = 0u32;
    let mut shown = Vec::new();
    let mut left_for_dolk = false;
    for _ in 0..1500u32 {
        let pan = s
            .host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .chain(s.host.world.cutscene.timeline.as_ref())
            .find(|tl| {
                tl.bytecode
                    .windows(7)
                    .any(|w| w == [0xCC, 0xF8, 0x51, 0x7F, 0x7F, 0x80, 0x02])
            })
            .map(|tl| tl.pc);
        let scene = s.host.scene.as_ref().map(|x| x.name.clone());
        if scene.as_deref() == Some("dolk") {
            left_for_dolk = true;
            break;
        }
        if let Some(pc) = pan
            && pc > 0x30
        {
            pan_frames += 1;
            let p = s
                .host
                .world
                .player_actor_slot
                .and_then(|i| s.host.world.actors.get(i as usize))
                .map(|a| (a.move_state.world_x, a.move_state.world_z));
            if p != Some((HIDE_XZ, HIDE_XZ)) {
                shown.push((pc, p));
            }
        }
        s.host.world.set_pad(0);
        s.tick().expect("tick");
    }
    assert!(
        pan_frames > 600,
        "the pan record runs ({pan_frames} frames)"
    );
    assert!(
        shown.is_empty(),
        "Vahn stands in the hide box under the pan: {shown:x?}"
    );
    assert!(left_for_dolk, "the pan hands off to Drake Castle");
}
