//! The Spirit charge's aura, against the real disc: the Spirit clip's effect
//! script spawns the table-form prototypes `0x07` / `0x08` (PROT 0898's
//! `0x801F6324` table), whose move-VM programs arm a VDF morph lane on
//! `vdf.dat` (PROT 0872) entry 12 and hold it for `0x4F` frames. The part has
//! to (a) survive that hold - the wait timer drains at retail's per-frame
//! rate, not the scene-graph step - and (b) draw its **morphed** mesh, which
//! is what turns a small rest mesh into the cone around the actor.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::move_power::MovePowerCatalog;
use legaia_engine_core::world::{GlobalTmd, SceneMode, World};
use legaia_prot::archive::Archive;

fn extracted() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file())
}

fn entry(archive: &mut Archive, idx: usize) -> Vec<u8> {
    let e = archive.entries.get(idx).cloned().expect("PROT entry");
    let mut bytes = Vec::new();
    archive.read_entry(&e, &mut bytes).expect("read entry");
    bytes
}

fn extent(g: &GlobalTmd) -> i32 {
    g.tmd
        .objects
        .iter()
        .flat_map(|o| o.vertices.iter())
        .map(|v| i32::from(v.x).abs().max(i32::from(v.z).abs()))
        .max()
        .unwrap_or(0)
}

#[test]
fn the_spirit_aura_prototypes_hold_and_draw_their_morphed_cone() {
    let Some(dir) = extracted() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut archive = Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let overlay = entry(
        &mut archive,
        legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX,
    );
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.tables.move_power = Some(MovePowerCatalog::from_overlay_0898(&overlay).expect("catalog"));
    world.tables.move_power_overlay = Some(Arc::from(overlay.as_slice()));
    world.tables.battle_vdf = Some(Arc::from(entry(&mut archive, 872).as_slice()));
    // The effect-model library (PROT 0871) at pool `3..`, as scene entry
    // seeds it. The pack spans the entry's extended footprint, so read the
    // next entry's bytes on too.
    let mut etmd = entry(&mut archive, 871);
    etmd.extend(entry(&mut archive, 872));
    for (i, body) in legaia_asset::pack::extract_pack(&etmd)
        .expect("etmd pack")
        .iter()
        .enumerate()
        .take(30)
    {
        if let Ok(tmd) = legaia_tmd::parse(body) {
            world.set_global_tmd(
                3 + i,
                Arc::new(GlobalTmd {
                    tmd,
                    raw: body.to_vec(),
                }),
            );
        }
    }

    assert!(world.spawn_action_table_effect(0x07, [0, 0, -800]));
    let mut grown = None;
    for tick in 0..30 {
        world.tick_move_fx(legaia_engine_core::world::EFFECT_SCENE_GRAPH_STEP);
        let draws = world.active_move_fx_part_draws();
        assert_eq!(draws.len(), 1, "the aura part is still up at tick {tick}");
        let d = draws[0];
        let rest = world
            .global_tmd(d.model_index as i16)
            .cloned()
            .expect("the aura's rest mesh is in the pool");
        if let Some(m) = world.morphed_part_tmd(&d) {
            grown = Some((extent(&rest), extent(&m)));
        }
    }
    let (rest, morphed) = grown.expect("the aura part armed a morph lane");
    assert!(
        morphed > rest * 2,
        "vdf.dat entry 12 grows the cone: rest extent {rest}, morphed {morphed}"
    );
}
