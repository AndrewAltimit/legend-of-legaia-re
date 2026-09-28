//! Disc-gated: the scene-load **field CD-XA prestage list** against every
//! CDNAME scene.
//!
//! Field-VM op `0x36` with bit 15 of its first operand clear and a non-zero
//! selector is the XA arm (`0x801E0420`): `FUN_8003D53C(arg >> 3, arg & 7,
//! sel)`. `SceneHost::load_scene` lists every such op of the scene MAN
//! (`field_xa::scene_xa_prestage`) so a host that decodes clips
//! asynchronously - the browser page - can stage them before the op fires.
//!
//! The census must be non-empty (a list that is empty on the whole disc is
//! decoration), every entry must name one of the `XA*.XA` files a clip slot
//! reads (`XA<slot + 1>.XA`; the disc ships `XA1.XA` to `XA34.XA`, slots `0..=33`), and every channel must be one of
//! the eight a sector interleaves. A scene with no MAN lists nothing.
//!
//! Skips (and passes) without `extracted/` or `LEGAIA_DISC_BIN`.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn every_scene_lists_its_field_xa_one_shots_at_load() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut scenes_with = 0usize;
    let mut clips = 0usize;
    let mut loaded = 0usize;
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        loaded += 1;
        let list = host.world.drain_field_xa_prestage();
        if list.is_empty() {
            continue;
        }
        scenes_with += 1;
        clips += list.len();
        for c in &list {
            assert!(c.clip < 34, "{name}: clip slot {} names no XA file", c.clip);
            assert!(c.channel < 8, "{name}: channel {} out of range", c.channel);
            assert!(
                c.duration_sectors > 0,
                "{name}: a zero selector is the seek-ahead"
            );
        }
        // Deduplicated.
        for (i, a) in list.iter().enumerate() {
            assert!(
                !list[i + 1..].contains(a),
                "{name}: duplicate prestage entry"
            );
        }
        // Drained: the list is taken, not copied.
        assert!(host.world.drain_field_xa_prestage().is_empty());
    }
    eprintln!(
        "[ok] field XA prestage: {clips} clips across {scenes_with} of {loaded} loaded scenes"
    );
    assert!(loaded > 0, "no scene loaded");
    assert!(
        scenes_with > 0,
        "no scene on the disc lists a field XA one-shot"
    );
}
