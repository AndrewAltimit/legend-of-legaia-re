//! Disc-gated: the committed borrowed-mesh catalog
//! (`legaia_expansion_format::mesh_catalog`) matches the disc - every
//! catalogued scene's geometry pack has exactly the catalogued objects, and
//! each one's measured size, `solid` and triangle count are the file's.
//! Regenerate with `legaia-engine mesh-catalog --catalog ...` when the
//! measurement changes. Skips when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_export::mesh_catalog::measure_scene;
use legaia_engine_scene::scene::ProtIndex;
use legaia_engine_scene::vfs::{DiscVfs, Vfs};
use legaia_expansion_format::mesh_catalog::catalog;

#[test]
fn the_committed_mesh_catalog_matches_the_disc() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let vfs = DiscVfs::open(std::path::Path::new(&disc)).expect("open the disc");
    let prot = vfs.read("prot.dat").expect("PROT.DAT");
    let cdname = vfs
        .read("cdname.txt")
        .ok()
        .map(|b| String::from_utf8(b).unwrap());
    let index = ProtIndex::from_bytes(prot, cdname.as_deref()).expect("index");
    let cat = catalog();
    let mut bad = Vec::new();
    for scene in cat.scenes() {
        let pack = measure_scene(&index, scene).expect("measure");
        let listed = cat.iter().filter(|e| e.scene == scene).count();
        if listed != pack.meshes.len() {
            bad.push(format!(
                "{scene}: {listed} catalogued, {} on disc",
                pack.meshes.len()
            ));
        }
        for m in &pack.meshes {
            match cat.get(scene, m.object) {
                Some(e) if e.size == m.size && e.solid == m.solid && e.tris == m.tris => {}
                Some(e) => bad.push(format!(
                    "{scene}:{} catalogued {:?}/{}/{} measured {:?}/{}/{}",
                    m.object, e.size, e.solid, e.tris, m.size, m.solid, m.tris
                )),
                None => bad.push(format!("{scene}:{} not catalogued", m.object)),
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
