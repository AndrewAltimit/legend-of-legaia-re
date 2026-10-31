//! Disc-gated: the committed NPC posture table
//! (`legaia-expansion-format`'s `data/npc_postures.toml`) is what
//! `npc_posture::measure_scene` measures off the disc, scene by scene.
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_expansion_format::npc_postures::table;
use legaia_web_expansion::catalog::DiscCatalog;

#[test]
fn the_committed_posture_table_matches_the_disc() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let image = std::fs::read(path).expect("read the disc");
    let cat = DiscCatalog::open(&image).expect("open the disc");
    let committed = table();
    for scene in committed.scenes().keys() {
        let measured: Vec<_> = legaia_web_expansion::npc_posture::measure_scene(&cat.index, scene)
            .expect("measure")
            .iter()
            .map(|p| p.postures(scene))
            .collect();
        let rows: Vec<_> = committed
            .npc
            .iter()
            .filter(|p| p.scene == *scene)
            .cloned()
            .collect();
        assert_eq!(
            rows, measured,
            "{scene}: regenerate with `cargo run -p legaia-web-expansion --example npc_postures`"
        );
    }
    // The scene picker's NPC list carries the same rows for the form hint.
    let npcs = cat.scene_npcs("koin1").expect("koin1 npcs");
    assert!(npcs.postures.iter().any(|p| !p.seated.is_empty()));
}
