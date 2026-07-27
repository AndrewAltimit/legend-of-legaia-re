//! Disc-gated: the browser field-scene animation runner
//! (`field_scene::build_field_scene_anim` / `FieldSceneAnim::tick`) animates
//! the two mechanism families end-to-end against the real disc, exactly as
//! the site page drives it (init after assembly, one vsync per rendered
//! frame, VRAM re-upload on change).
//!
//!  - `jou`: no walker table, but the ambient move-VM tree spawns (the
//!    pulsating-flesh palette cyclers) and ticking rewrites VRAM texels.
//!  - `garmel`: a 1-entry walker table (water shimmer) whose `MoveImage`
//!    fires change the dest CLUT cell.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_scene::{build_field_scene, build_field_scene_anim};
use std::env;
use std::fs;

fn index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex"))
}

#[test]
fn jou_and_garmel_animate_in_the_viewer_or_skip() {
    let Some(index) = index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };

    // jou: ambient move-VM tree only.
    let mut pack = build_field_scene(&index, "jou").expect("build jou");
    let mut anim = build_field_scene_anim(&index, &mut pack).expect("jou has animation sources");
    let (walkers, ambient) = anim.status();
    assert_eq!(walkers, 0, "jou has no walker table");
    assert!(ambient >= 20, "jou ambient fan-out ({ambient})");
    let before = pack.res.vram.as_bytes().to_vec();
    let mut wrote = false;
    for _ in 0..16 {
        wrote |= anim.tick(1, &mut pack.res.vram);
    }
    assert!(wrote, "jou ambient tick reports VRAM changes");
    assert_ne!(
        before,
        pack.res.vram.as_bytes(),
        "jou VRAM texels actually changed"
    );

    // garmel: 1-entry walker table.
    let mut pack = build_field_scene(&index, "garmel").expect("build garmel");
    let mut anim = build_field_scene_anim(&index, &mut pack).expect("garmel has animation sources");
    let (walkers, _) = anim.status();
    assert_eq!(walkers, 1, "garmel walker entries");
    let mut wrote = false;
    for _ in 0..32 {
        wrote |= anim.tick(1, &mut pack.res.vram);
    }
    assert!(wrote, "garmel walker fires MoveImage copies");
}
