//! The field scene's actor-pool parts are off retail's lists during a
//! battle (`World::field_pool_drawn`): the per-stage init `FUN_8001E1B4`
//! re-seeds the actor pool on the way in, so the field's ambient tree and
//! stager parts draw nothing there, and come back with the parked pool on
//! the return. `map01`'s mist puffs, kept live through a fight, stacked into
//! a white bar on the horizon of Kemaro's low camera.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// An ambient `0x4000` sprite-arm node: one blended quad.
fn mist_sheet() -> crate::world::ambient::AmbientPart {
    let mut s = legaia_engine_vm::move_vm::ActorState {
        move_substate: legaia_engine_effects::effect_sprite_arm::DRAW_KIND_MULTI as i16,
        field_9e: legaia_engine_effects::effect_sprite_arm::SPRITE_ARM_BIT,
        field_72: 0x1000,
        field_74: 0xC100_0000,
        ..Default::default()
    };
    s.set_actor_u16(0xB4, 0x40);
    s.set_actor_u16(0xB6, 0x20);
    s.set_actor_u16(0xAC, 0x3F);
    s.set_actor_u16(0xAE, 0x1F);
    s.set_actor_u16(0xB0, 0x0015);
    s.set_actor_u16(0xB2, 0x7DC0);
    s.set_actor_u32(0xA0, 0x0080_8080);
    crate::world::ambient::AmbientPart {
        record_off: 0,
        model_sel: -1,
        reserved: 0,
        buf: Vec::new(),
        state: s,
        finished: false,
        cell_fx: None,
        scroll_fx: Vec::new(),
        prev_morph_weights: Default::default(),
    }
}

#[test]
fn field_ambient_parts_draw_on_the_field_and_not_in_a_battle() {
    let mut w = World::new();
    w.ambient.fx.push(mist_sheet());
    w.mode = SceneMode::Field;
    assert!(w.field_pool_drawn());
    assert_eq!(w.active_effect_kind4_draws().len(), 1, "the field draws it");
    w.mode = SceneMode::Battle;
    assert!(!w.field_pool_drawn());
    assert!(
        w.active_effect_kind4_draws().is_empty(),
        "the battle's re-seeded pool holds no field part"
    );
    assert!(w.active_field_fx_part_draws().is_empty());
    assert_eq!(w.ambient.fx.len(), 1, "kept for the return to the field");
    w.mode = SceneMode::Field;
    assert_eq!(
        w.active_effect_kind4_draws().len(),
        1,
        "back after the battle"
    );
}
