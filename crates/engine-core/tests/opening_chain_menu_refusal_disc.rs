//! The pause menu stays shut through the New Game opening chain, on the
//! engine's own predicate.
//!
//! The opening (`opdeene` -> `opstati` -> `opurud` -> `town01`) is cutscene
//! timelines and narration crawls end to end. Retail's menu-open accept is a
//! leg of the pad controller `FUN_801D01B0`, which `FUN_801D1344` calls only
//! when the player's engaged bit `+0x10 & 0x80000` is clear (`0x801D1694`),
//! and the per-actor script runner `FUN_80039B7C` holds that bit for every
//! frame it steps a context; the crawls own the screen besides. So no frame
//! of the chain reaches the accept.
//!
//! [`World::field_menu_open_allowed`](legaia_engine_core::world::World::field_menu_open_allowed)
//! is the one rule every host asks. This pins that it answers "no" on every
//! tick the chain runs. Without its `opening_chain_active` arm it admitted
//! one tick per leg - the tick each of `opstati` and `opurud` loads on, one
//! before the engine seats the leg's entry record - and only the browser play
//! page refused it, through a page-side copy of the gate the native window
//! did not have.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn the_opening_chain_never_admits_the_pause_menu() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.begin_new_game();
    host.enter_field_scene(legaia_asset::new_game::OPENING_CUTSCENE_SCENE, 0)
        .expect("enter opdeene");
    assert!(
        host.world.cutscene.opening_chain_active,
        "opdeene arms the chain"
    );

    let mut chain_ticks = 0usize;
    let mut admitted = Vec::new();
    let mut scenes = Vec::<String>::new();
    for tick in 0..40_000usize {
        host.world.set_pad(0);
        host.tick().expect("tick");
        let scene = host
            .scene
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if scenes.last() != Some(&scene) {
            scenes.push(scene.clone());
        }
        if !host.world.cutscene.opening_chain_active {
            break;
        }
        chain_ticks += 1;
        if host.world.field_menu_open_allowed() {
            admitted.push((tick, scene));
        }
    }
    eprintln!(
        "[ran] opening chain: {chain_ticks} ticks over {scenes:?}; admitted {}",
        admitted.len()
    );
    assert!(chain_ticks > 1000, "the chain ran ({chain_ticks} ticks)");
    assert!(
        !host.world.cutscene.opening_chain_active,
        "the chain ends on its own (scenes {scenes:?})"
    );
    assert!(
        admitted.is_empty(),
        "the menu-open predicate admitted {} chain tick(s), first {:?}",
        admitted.len(),
        admitted.first()
    );
}
