//! Disc-gated: the overworld map display seats, spins and retires the
//! player's **map pointer**, and both hosts' draw list carries it.
//!
//! The display's mode `1` (`0x801CE8D8`, PROT 0981) patches its spawn record
//! at `0x801CF6E8` with the last TMD id the scene load issued
//! (`0x8007BB38 - 0x8007BA6C`) and seats it on the leader through
//! `FUN_80021B04`; the tick's tail calls the part's handler every frame and
//! mode `4` raises its halt bit. In the library's kingdom states
//! `0x8007BB38` is `44` on `map01` and `60` on `map03`, and
//! `DAT_8007C018[0x8007BB38]` is the last TMD of the kingdom bundle's pack -
//! a 14-vertex mesh all three kingdoms carry byte for byte.
//!
//! The chain asserted is the one both play hosts run: L1 through the
//! world's pad, `World::tick` -> `tick_map_display` -> the pointer's part
//! tick, and `effect_draws::effect_model_draws`'s `field_parts` - the list
//! the native field-FX pass and the page's `build_field_fx` both draw.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_field::map_display::MODE_INTERACTIVE;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::effect_draws::effect_model_draws;
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

fn frame(host: &mut SceneHost, pad: u16) {
    host.world.set_pad(pad);
    host.tick().expect("tick");
}

/// The overworld's field-part draws whose mesh is the pack slot `slot`.
fn pointer_draws(
    host: &SceneHost,
    slot: usize,
) -> Vec<legaia_engine_world_views::effect_draws::FxPartModel> {
    effect_model_draws(&host.world, None, |_| None)
        .field_parts
        .into_iter()
        .filter(|p| p.draw.model_index == slot)
        .collect()
}

#[test]
fn the_map_display_draws_a_spinning_pointer_on_the_player() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    let mut meshes: Vec<Vec<u8>> = Vec::new();
    for scene in ["map01", "map02", "map03"] {
        host.enter_world_map_scene(scene).expect("enter kingdom");
        for _ in 0..30 {
            frame(&mut host, 0);
        }
        let src = host
            .world
            .world_map
            .pointer_source
            .clone()
            .unwrap_or_else(|| panic!("{scene}: the pointer source is installed"));
        // The slot the patched selector names is the pack's last, and its
        // mesh is the shared pointer.
        let pack = {
            let scene_ref = host.scene.as_ref().expect("scene");
            let res = host.resources.as_ref().expect("resources");
            legaia_engine_scene::field_env::env_pack_tmd_indices(scene_ref, res)
                .into_iter()
                .map(|i| {
                    let t = &res.tmds[i];
                    let colour = legaia_tmd::mesh::tmd_to_color_mesh(&t.tmd, &t.raw);
                    let textured = legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw);
                    (
                        t.raw.clone(),
                        t.tmd.stats().total_vertices,
                        colour.indices.len(),
                        textured.indices.len(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(src.pack_slot, pack.len() - 1, "{scene}: the last pack slot");
        let (mesh, verts, colour_ix, textured_ix) = pack[src.pack_slot].clone();
        assert_eq!(verts, 14, "{scene}: the pointer mesh");
        // All untextured: the field part pass must draw the colour half.
        assert!(colour_ix > 0, "{scene}: the pointer has untextured prims");
        assert_eq!(textured_ix, 0, "{scene}: and no textured ones");
        meshes.push(mesh);

        // The walk frame draws no pointer.
        assert!(
            pointer_draws(&host, src.pack_slot).is_empty(),
            "{scene}: no pointer before the display opens"
        );

        frame(&mut host, PadButton::L1 as u16);
        let mut opened = false;
        for _ in 0..120 {
            frame(&mut host, 0);
            if host
                .world
                .world_map
                .map_display
                .as_ref()
                .is_some_and(|d| d.mode == MODE_INTERACTIVE)
            {
                opened = true;
                break;
            }
        }
        assert!(opened, "{scene}: L1 never reached the interactive display");

        let draws = pointer_draws(&host, src.pack_slot);
        assert_eq!(draws.len(), 1, "{scene}: one pointer draws on the display");
        let slot = host.world.player_actor_slot.expect("player") as usize;
        let m = &host.world.actors[slot].move_state;
        let p = draws[0].draw.world_pos;
        assert_eq!(
            (p[0], p[1], p[2]),
            (
                f32::from(m.world_x),
                f32::from(m.world_y),
                f32::from(m.world_z)
            ),
            "{scene}: the pointer stands on the leader"
        );
        assert_eq!(draws[0].draw.scale, 0x1000, "{scene}: the spawn's scale");
        // `0C FF89 0000 0000 0000 0001`: an additive (ABR 1), semi-
        // transparent, double-sided colour word.
        let c = draws[0].draw.colour;
        assert!(c.semi && c.abr == 1 && c.double_sided, "{scene}: {c:?}");

        // The program spins it (`04 0000 0020 0000`: `+0x82 = 0x100`), so the
        // pose moves from frame to frame while nothing else does.
        let before = draws[0].draw.rot;
        frame(&mut host, 0);
        let after = pointer_draws(&host, src.pack_slot)[0].draw.rot;
        assert_ne!(before, after, "{scene}: the pointer turns");

        // Circle leaves: the fade out, mode 4's halt, the hand-back.
        frame(&mut host, PadButton::Circle as u16);
        let mut closed = false;
        for _ in 0..60 {
            frame(&mut host, 0);
            if host.world.world_map.map_display.is_none() {
                closed = true;
                break;
            }
        }
        assert!(closed, "{scene}: Circle never closed the display");
        assert!(host.world.world_map.pointer.is_none(), "{scene}: retired");
        assert!(
            pointer_draws(&host, src.pack_slot).is_empty(),
            "{scene}: no pointer after the display closes"
        );
    }
    assert!(
        meshes.windows(2).all(|w| w[0] == w[1]),
        "the three kingdoms share one pointer mesh"
    );
}
