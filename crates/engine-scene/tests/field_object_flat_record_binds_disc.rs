//! Disc-gated: a placed object's trigger names a **flat** MAN record.
//!
//! `FUN_8003A55C` sums the three partition counts (`0x8003A800..0x8003A848`)
//! and indexes one table of 24-bit record offsets with the trigger's record
//! byte (`0x8003A854..0x8003A878`), so a record past partition 0 binds like
//! any other. `urudre3`'s partition 0 holds `28` records and the trigger on
//! tile `(86, 106)` names record `28`, the first of partition 1: the
//! `urudre3_songi_room` retail state holds that actor (`+0x50 = 28`,
//! `+0x9E = 8`, draw kind `5`) drawing the wardrobe at `(11072, 13632)`.
//! Looked up in partition 0 alone, the object was dropped as unbound and the
//! wardrobe never drew.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use crate::scene::{ProtIndex, Scene};

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
fn a_trigger_past_partition_zero_binds_its_flat_record() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open prot index");
    let scene = Scene::load(&index, "urudre3").expect("urudre3");
    let man = scene
        .field_man_payload(&index)
        .expect("man")
        .expect("urudre3 MAN");
    let mf = legaia_scene_formats::man_section::parse(&man).expect("parse MAN");
    assert_eq!(
        mf.partitions[0].len(),
        28,
        "partition 0 ends before record 28"
    );
    let binds = scene
        .field_object_binds(&index)
        .expect("binds")
        .expect("urudre3 binds");
    let b = binds.get(&(86, 106)).expect("the wardrobe's tile binds");
    assert_eq!((b.record, b.anim_id), (28, 0));
    // The partition-0 binds are unchanged: the cupboard beside it.
    let c = binds.get(&(86, 98)).expect("record 27 binds");
    assert_eq!((c.record, c.anim_id), (27, 3));
}
