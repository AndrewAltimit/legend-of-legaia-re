//! Field-VM `4C DB` - the single-source CLUT blend fade (`FUN_801E57F0`
//! spawns descriptor `0x801F2930`, whose handler is `FUN_801E4D8C`) - driven
//! through `World::spawn_clut_blend_fx` + `World::step_clut_fx` against a
//! scratch software VRAM. Disc-free: the record bytes are hand-authored in
//! the shape jouine's MAN carries (`[DB, x, y, R, G, B, frac, duration]`).

use legaia_engine_core::world::World;
use legaia_engine_vm::world_map_clut_fade::{ClutBlendFade, FadeRecord, FadeStep};

fn record(x: u16, y: u16, rgb: [u8; 3], frac: u16, duration: u16) -> [u8; 12] {
    let mut r = [0u8; 12];
    r[0] = 0xDB;
    r[1..3].copy_from_slice(&x.to_le_bytes());
    r[3..5].copy_from_slice(&y.to_le_bytes());
    r[5..8].copy_from_slice(&rgb);
    r[8..10].copy_from_slice(&frac.to_le_bytes());
    r[10..12].copy_from_slice(&duration.to_le_bytes());
    r
}

fn put_row(vram: &mut legaia_tim::Vram, x: u16, y: u16, row: &[u16; 16]) {
    let mut bytes = [0u8; 32];
    for (i, v) in row.iter().enumerate() {
        bytes[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
    }
    vram.write_clut_row(x, y, &bytes);
}

fn get_row(vram: &legaia_tim::Vram, x: u16, y: u16) -> [u16; 16] {
    std::array::from_fn(|i| vram.pixel(x as usize + i, y as usize))
}

#[test]
fn zero_duration_record_lands_the_endpoint_on_the_first_game_tick() {
    let bytes = record(0x20, 0x1F6, [0x1E, 0x14, 0x5A], 0x04B0, 0);
    let src: [u16; 16] = std::array::from_fn(|i| 0x7FFF - i as u16 * 0x0421);
    let mut vram = legaia_tim::Vram::new();
    put_row(&mut vram, 0x20, 0x1F6, &src);

    let mut expect = ClutBlendFade::new(&src, &FadeRecord::from_bytes(&bytes));
    let FadeStep::Done(want) = expect.tick(2) else {
        panic!("a zero-duration record completes on its first tick");
    };

    let mut world = World::new();
    world.clock.frame_step = 2;
    world.spawn_clut_blend_fx(&bytes);
    world.ambient.clut_pending_game_ticks = 1;
    assert!(world.step_clut_fx(&mut vram), "the fade wrote VRAM");
    assert_eq!(get_row(&vram, 0x20, 0x1F6), want);
    assert!(
        world.ambient.clut_blend_fx.is_empty(),
        "a Done fade retires"
    );
}

#[test]
fn a_timed_fade_steps_per_game_tick_and_retires_at_its_duration() {
    let bytes = record(0x40, 0x1F0, [0xF8, 0, 0], 0x1000, 8);
    let src = [0u16; 16];
    let mut vram = legaia_tim::Vram::new();
    put_row(&mut vram, 0x40, 0x1F0, &src);

    let mut world = World::new();
    world.clock.frame_step = 2;
    world.spawn_clut_blend_fx(&bytes);
    // Three game ticks at dt = 2: acc 2, 4, 6 < 8 -> still live.
    world.ambient.clut_pending_game_ticks = 3;
    assert!(world.step_clut_fx(&mut vram));
    assert_eq!(world.ambient.clut_blend_fx.len(), 1);
    let mid = get_row(&vram, 0x40, 0x1F0);
    assert!(
        mid[0] & 0x1F > 0 && mid[0] & 0x1F < 0x1F,
        "red part-way: {:#06x}",
        mid[0]
    );
    // The fourth reaches acc = 8 and lands the endpoint (full red).
    world.ambient.clut_pending_game_ticks = 1;
    assert!(world.step_clut_fx(&mut vram));
    assert!(world.ambient.clut_blend_fx.is_empty());
    assert_eq!(get_row(&vram, 0x40, 0x1F0)[0] & 0x1F, 0x1F);
}

#[test]
fn a_short_record_is_dropped() {
    let mut world = World::new();
    world.spawn_clut_blend_fx(&[0xDB, 0, 0]);
    assert!(world.ambient.clut_blend_fx.is_empty());
}
