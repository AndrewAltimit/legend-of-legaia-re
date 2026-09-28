//! Disc-gated: which shipped move programs reach the render dispatcher's
//! `0x4000` sprite arm, and that each one lands on the draw list both hosts'
//! FX passes draw (`World::active_effect_kind4_draws`).
//!
//! Every slot-B cast / summon image (`0903..=0966`) is staged through
//! `World::spawn_summon` in battle and ticked; a part whose state reaches
//! draw kind `4` with `+0x9E & 0x4000` (move-VM op `0x23`) must come back
//! from the draw list as one textured quad. The test reports the carriers
//! rather than trusting a hand list, and pins only the chain *move program
//! -> actor fields -> quad*, no Sony bytes.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::world::{SceneMode, World};
use legaia_prot::archive::Archive;

fn prot() -> Option<PathBuf> {
    for b in ["extracted", "../../extracted", "../extracted"] {
        let p = PathBuf::from(b).join("PROT.DAT");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[test]
fn shipped_sprite_arm_nodes_reach_the_kind4_draw_list() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(prot) = prot() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let mut carriers = Vec::new();
    for idx in 903usize..=966 {
        let entry = archive.entries[idx].clone();
        let mut bytes = Vec::new();
        if archive.read_entry(&entry, &mut bytes).is_err() {
            continue;
        }
        let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
        if overlay.parts.is_empty() {
            continue;
        }
        let mut world = World::default();
        world.enter_battle(3, 2);
        assert_eq!(world.mode, SceneMode::Battle);
        world.spawn_summon(&overlay, &bytes, 0, [0, 0, 0]);
        let mut peak = 0usize;
        let mut default_peak = 0usize;
        let mut pages = std::collections::BTreeSet::new();
        for _ in 0..600 {
            world.tick_summon(8);
            let Some(scene) = world.casting.active_summon.as_ref() else {
                break;
            };
            // The scene's own sprite-arm and default-arm nodes, and the one
            // list both hosts draw, which must carry every one of them.
            let sprites = scene.sprite_arm_draws();
            let defaults = scene.default_arm_draws();
            let listed = world.active_effect_kind4_draws().len();
            assert!(
                listed >= sprites.len() + defaults.len(),
                "PROT {idx:04}: the kind-4 list drops a node"
            );
            for d in &sprites {
                assert_eq!(d.mesh.positions.len(), 4, "PROT {idx:04}: one quad");
                assert_eq!(d.mesh.indices, vec![0, 1, 2, 2, 1, 3]);
                pages.insert((d.mesh.cba_tsb[0][1] & 0x7F9F, d.mesh.cba_tsb[0][0]));
            }
            peak = peak.max(sprites.len());
            default_peak = default_peak.max(defaults.len());
        }
        if default_peak > 0 {
            eprintln!("[ok] PROT {idx:04}: peak {default_peak} default-arm meshes");
        }
        if peak > 0 {
            eprintln!("[ok] PROT {idx:04}: peak {peak} sprite quads, (tpage, clut) {pages:x?}");
            carriers.push(idx);
        }
    }
    eprintln!("sprite-arm carriers reached through the summon spawner: {carriers:?}");
    assert!(
        !carriers.is_empty(),
        "no slot-B image reached the sprite arm - op 0x23's stores or the draw list regressed"
    );
}
