//! Page ladder for the overworld map display's **pointer**: the browser play
//! page draws the move-VM part PROT 0981's mode `1` seats on the leader, the
//! same `field_parts` draw the native window's field-FX pass takes.
//!
//! The chain is the page's own: `tick_frame` (the world's L1 fade-up and
//! `World::tick_map_display`, which seats and ticks the part), then
//! `play_field_fx_sync` against the engine camera, whose model list resolves
//! the part to the kingdom pack's last mesh - the pointer
//! (`docs/subsystems/world-map.md`, the map display section). The pointer's
//! program sets a colour word (`0C FF89 0000 0000 0000 0001`: ABE, additive
//! ABR, double-sided), so the page bakes it into the FX stream, and its mesh
//! is all untextured prims, which ride that stream's flat-colour arm.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_field::map_display::MODE_INTERACTIVE;
use legaia_engine_system::input::PadButton;
use legaia_web_play::runtime::LegaiaRuntime;

const W: f32 = 320.0;
const H: f32 = 240.0;

/// One page frame: the tick, then the field FX pass the page runs after it.
fn frame(rt: &mut LegaiaRuntime, pad: u16) {
    rt.set_pad(pad);
    rt.tick_frame().expect("tick_frame");
    let vp = rt.play_camera_vp(W, H);
    rt.play_field_fx_sync(&vp);
}

/// The FX model draws that index the scene pack's slot `slot`.
fn pack_draws(rt: &LegaiaRuntime, slot: u32) -> usize {
    (0..rt.play_battle_fx_model_count())
        .filter(|&i| {
            rt.play_battle_fx_model_is_scene_pack(i) && rt.play_battle_fx_model_tmd(i) == slot
        })
        .count()
}

#[test]
fn the_page_draws_the_map_display_pointer() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("map01").expect("enter map01");
    assert_eq!(rt.scene_mode(), "WorldMap", "map01 is the overworld");
    // The scene picker arms the developer band, which shadows the retail L1
    // arm (`World::arm_picker_world_map_debug`); a door or a Load leaves it
    // off, which is the entry a player makes.
    if let Some(c) = rt
        .scene_host
        .host_mut()
        .and_then(|h| h.world.world_map.ctrl.as_mut())
    {
        c.debug_enabled = false;
    }
    for _ in 0..30 {
        frame(&mut rt, 0);
    }
    let slot = {
        let w = rt.scene_host.host().map(|h| &h.world).expect("world");
        w.world_map
            .pointer_source
            .as_ref()
            .expect("the kingdom installs the pointer source")
            .pack_slot as u32
    };
    assert_eq!(pack_draws(&rt, slot), 0, "no pointer on the walk frame");

    frame(&mut rt, PadButton::L1.mask());
    let mut opened = false;
    for _ in 0..120 {
        frame(&mut rt, 0);
        if rt
            .scene_host
            .host()
            .and_then(|h| h.world.world_map.map_display.as_ref())
            .is_some_and(|d| d.mode == MODE_INTERACTIVE)
        {
            opened = true;
            break;
        }
    }
    assert!(opened, "L1 never reached the interactive display");
    // The page draws the part one of two ways: a pack-mesh model, or - with
    // a colour word of its own, as this program sets (`0C ...`) - baked into
    // the FX stream. Measure the part's own share of this frame's page draw
    // by building the pass with and without it.
    let vp = rt.play_camera_vp(W, H);
    rt.play_field_fx_sync(&vp);
    let with = (
        pack_draws(&rt, slot),
        rt.play_battle_fx_positions().len() / 3,
    );
    let part = rt
        .scene_host
        .host_mut()
        .and_then(|h| h.world.world_map.pointer.take())
        .expect("the display seated the pointer");
    rt.play_field_fx_sync(&vp);
    let without = (
        pack_draws(&rt, slot),
        rt.play_battle_fx_positions().len() / 3,
    );
    if let Some(h) = rt.scene_host.host_mut() {
        h.world.world_map.pointer = Some(part);
    }
    let baked_verts = {
        let h = rt.scene_host.host().expect("host");
        let scene = h.scene.as_ref().expect("scene");
        let res = h.resources.as_ref().expect("resources");
        let pack = legaia_engine_scene::field_env::env_pack_tmd_indices(scene, res);
        let t = &res.tmds[pack[slot as usize]];
        legaia_tmd::mesh::tmd_to_vram_mesh(&t.tmd, &t.raw)
            .positions
            .len()
            + legaia_tmd::mesh::tmd_to_color_mesh(&t.tmd, &t.raw)
                .positions
                .len()
    };
    assert!(baked_verts > 0, "the pointer mesh has geometry");
    assert!(
        with.0 == without.0 + 1 || with.1 == without.1 + baked_verts,
        "the page draws the pointer mesh: with {with:?}, without {without:?}, \
         mesh {baked_verts} vertices"
    );

    frame(&mut rt, PadButton::Circle.mask());
    for _ in 0..60 {
        frame(&mut rt, 0);
        if rt
            .scene_host
            .host()
            .map(|h| &h.world)
            .is_some_and(|w| w.world_map.map_display.is_none())
        {
            break;
        }
    }
    assert!(
        rt.scene_host
            .host()
            .map(|h| &h.world)
            .is_some_and(|w| w.world_map.pointer.is_none()),
        "the pointer retires with the display"
    );
    assert_eq!(pack_draws(&rt, slot), 0, "no pointer after the display");
}
