//! The world's trig LUTs are filled at construction with retail's one SCUS
//! sine table, so move-VM op `0x03` and the world-map horizon emitter read
//! real samples in the live world rather than zero.
//!
//! Retail: `FUN_80026BE0` installs `_DAT_8007B81C = 0x80070A2C` (sine view)
//! and `_DAT_8007B7F8 = 0x80070A2C + 0x800` (cosine view); op `0x03` at
//! `0x80023184` reads the sine view for the X term (`lw v1,-0x47e4(v1)`) and
//! the cosine view for the Z term (`lw v1,-0x4808(v1)`), both indexed by the
//! same `& 0xFFF` angle and narrowed with `sra 0xc`.

use legaia_engine_core::action_effect_script::retail_rotation_lut;
use legaia_engine_core::world::World;

#[test]
fn world_new_fills_both_views_of_the_retail_table() {
    let w = World::new();
    assert_eq!(w.sin_lut.len(), 0x1000);
    assert_eq!(w.cos_lut.len(), 0x1000);
    // Cardinal points of `trunc(sin(i * 2pi / 4096) * 4096)`.
    assert_eq!(w.sin_lut[0], 0);
    assert_eq!(w.sin_lut[0x400], 4096);
    assert_eq!(w.sin_lut[0xC00], -4096);
    assert_eq!(w.cos_lut[0], 4096);
    assert_eq!(w.cos_lut[0x800], -4096);
    // The cosine view is the sine table a quarter revolution on.
    for i in 0..0x1000usize {
        assert_eq!(w.cos_lut[i], w.sin_lut[(i + 0x400) & 0xFFF], "i={i:#x}");
    }
    assert_eq!(&w.sin_lut[..], &retail_rotation_lut().sin_table()[..]);
}

#[test]
fn move_vm_op_03_moves_the_actor_in_the_live_world() {
    let mut w = World::new();
    let slot = 1;
    {
        let s = &mut w.actors[slot].move_state;
        s.wait_timer = -1;
        s.world_x = 1000;
        s.world_z = 2000;
        // Facing 0x400 = a quarter turn: sin = 4096, cos = 0.
        s.tween_scale_x = 0x400;
    }
    // op 0x03 (step 100), then HALT (0x08).
    let _ = w.actor_tick_at(slot, &[0x03, 100, 0x08], 8);
    let s = &w.actors[slot].move_state;
    assert_eq!(s.world_x, 1100, "X += (sin * 100) >> 12");
    assert_eq!(
        s.world_z, 2000,
        "Z += (cos * 100) >> 12 = 0 at a quarter turn"
    );

    let mut w = World::new();
    {
        let s = &mut w.actors[slot].move_state;
        s.wait_timer = -1;
        s.tween_scale_x = 0;
    }
    let _ = w.actor_tick_at(slot, &[0x03, 100, 0x08], 8);
    let s = &w.actors[slot].move_state;
    assert_eq!((s.world_x, s.world_z), (0, 100), "facing 0 steps along +Z");
}
