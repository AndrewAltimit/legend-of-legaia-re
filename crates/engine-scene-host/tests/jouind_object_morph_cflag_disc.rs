//! Disc-gated: a system-script `CFLAG_SET` aimed at a placed object lands
//! on the object's own context.
//!
//! `jouind`'s placed objects `9..=12` arm op-`0x4B` morphs in their bind
//! prologues and drop `+0x10` bit 12 behind them (`4B 01 .. 32 0C 2C 0C
//! 2B 0A 2C 0D`). The entry script `P1[0]`, with system flag `0x4C0` set and
//! `0x4C1` clear, raises it again on each (`B1 <id> 0C`, `AC <id> 0C`,
//! `AB <id> 0A`, `AB <id> 0D`), so the envelope runs held at peak and the
//! draw substitutes the morph (`FUN_8001ADA4` tests `+0x10 & 0x1000` before
//! `jal 0x8001C604` at `0x8001B43C`) - the flesh walls lie collapsed.
//! `jouind_field_card_boot` holds all four at `+0x10 = 0x08009881`, weight
//! `0x1000`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

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

#[test]
fn jouind_entry_script_rearms_the_wall_objects_morphs() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.world.system_flag_set(0x4C0);
    host.enter_field_scene("jouind", 0).expect("enter jouind");
    for _ in 0..120 {
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    for record in 9u16..=12 {
        let ch = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| c.object_bind && c.ctx.script_id == record)
            .expect("object bind channel");
        assert_ne!(
            ch.ctx.flags & 0x1000,
            0,
            "object {record}: the entry script's B1 {record:02X} 0C lands on the object"
        );
        let lanes = host
            .world
            .field_morph_lanes(legaia_engine_core::world::MorphOwner::Object(record))
            .expect("armed lanes");
        assert_eq!(lanes[0].1, 0x1000, "object {record} held at peak");
    }
}
