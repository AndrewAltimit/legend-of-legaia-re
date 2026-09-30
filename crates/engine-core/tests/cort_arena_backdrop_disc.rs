//! Disc-gated: the evolved-Cort arena (`jouine`, stage variant 2 = PROT 693)
//! after the PROT 0968 arrival hands the fight back.
//!
//! Battle init drops object 1 of the two-object stage shell, which leaves
//! object 0 - a wall textured from page 12 through CLUT row 473 (pink-brown
//! on the idle palette). The arrival's phase 6 then copies the actors'
//! object slot 1 over slot 0, so the shell draws **object 1 alone**: the
//! flesh shell on page 13 through CLUT `(32, 479)`, the dark-red palette the
//! arena shows in retail. The same phase `MoveImage`s an empty strip over the
//! ground grid's tile window, so the procedural floor samples only
//! transparent texels. Before this was ported the engine drew the pink wall
//! and a pink floor where retail is dark purple with red veins.
//!
//! Skips when `LEGAIA_DISC_BIN` / the extracted disc are missing
//! (`LEGAIA_EXTRACTED_DIR` first, then repo-relative).

use std::path::PathBuf;

use legaia_asset::battle_backdrop::{GROUND_TSB, ground_grid_drawable};
use legaia_engine_core::battle_stage_module::{ARRIVAL_GROUND_BLANK, StageEffect};
use legaia_engine_core::scene::{Scene, SceneHost};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};

fn extracted() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = ["extracted", "../extracted", "../../extracted"].map(PathBuf::from);
    let found = env
        .into_iter()
        .chain(rel)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    if found.is_none() {
        eprintln!("[skip] extracted/ missing");
    }
    found
}

/// The distinct `(CBA, TSB)` pairs the textured prims of `objects` sample.
fn sampled(tmd: &legaia_tmd::Tmd, raw: &[u8], objects: &[usize]) -> Vec<(u16, u16)> {
    let sub = legaia_asset::battle_backdrop::objects_tmd(tmd, objects);
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&sub, raw);
    let mut v: Vec<(u16, u16)> = mesh.cba_tsb.iter().map(|c| (c[0], c[1])).collect();
    v.sort_unstable();
    v.dedup();
    v
}

#[test]
fn the_cort_arena_draws_the_flesh_shell_after_the_arrival() {
    let Some(extracted) = extracted() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stage = host
        .index
        .battle_stage_entry_for_region("jouine", 2)
        .expect("jouine variant 2 names a stage stream");
    assert_eq!(stage, 693);
    let scene = Scene::load(&host.index, "jouine").expect("load jouine");
    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&host.index, n).ok())
        .collect();
    let refs: Vec<&Scene> = shared.iter().collect();
    let (res, _) = SceneResources::build_targeted_with_options(
        &scene,
        &refs,
        BuildOptions {
            kind: SceneLoadKind::Battle,
            upload_all_tims: true,
            system_ui: None,
        },
    )
    .expect("battle build");
    let dome = res
        .tmds
        .iter()
        .find(|t| t.entry_idx == stage)
        .expect("stage shell TMD");
    let n = dome.tmd.objects.len();
    assert_eq!(n, 2, "a two-object stage shell");

    // Battle init: object 0 alone, the page-12 wall through CLUT row 473.
    let before = host.battle_stage_object_indices(n);
    assert_eq!(before, vec![0]);
    let wall = sampled(&dome.tmd, &dome.raw, &before);
    assert!(
        wall.iter()
            .all(|&(cba, tsb)| cba >> 6 == 473 && tsb & 0xF == 12),
        "object 0 samples page 12 / CLUT row 473: {wall:x?}"
    );

    // The hand-back: object 1 alone, page 13 through CLUT (32, 479).
    host.world.battle.backdrop_rebound = true;
    let after = host.battle_stage_object_indices(n);
    assert_eq!(after, vec![1]);
    let flesh = sampled(&dome.tmd, &dome.raw, &after);
    assert_eq!(
        flesh,
        vec![(0x77C2, GROUND_TSB)],
        "object 1 = the flesh shell"
    );

    // The ground tile the grid samples is resident in the stage VRAM until
    // the hand-back's `MoveImage` blanks it.
    let mut vram = res.vram.clone();
    legaia_engine_core::scene::upload_battle_stage_tims_into_vram(&scene, stage, &mut vram);
    assert!(
        ground_grid_drawable(&vram),
        "the stage carries a ground tile"
    );
    let StageEffect::MoveImage {
        x,
        y,
        w,
        h,
        dst_x,
        dst_y,
    } = ARRIVAL_GROUND_BLANK
    else {
        unreachable!()
    };
    host.world
        .battle
        .vram_moves
        .push(legaia_engine_core::world::ScriptVramMove {
            src: (x as i16, y as i16),
            size: (w as i16, h as i16),
            dst: (dst_x as i16, dst_y as i16),
        });
    assert!(host.world.apply_battle_vram_moves(&mut vram));
    assert!(host.world.battle.vram_moves.is_empty(), "the queue drains");
    for row in dst_y..dst_y + h {
        for col in dst_x..dst_x + w {
            assert_eq!(vram.pixel(col as usize, row as usize), 0, "({col}, {row})");
        }
    }
    assert!(!ground_grid_drawable(&vram), "no floor after the hand-back");
}
