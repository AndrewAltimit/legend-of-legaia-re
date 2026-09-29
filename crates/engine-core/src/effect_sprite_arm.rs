//! The render dispatcher's `0x4000` **sprite arm** as a per-frame mesh, for
//! every move-VM part that becomes a draw-kind-4 sprite node (move-VM op
//! `0x23`): battle summon / move-FX / effect-script scenes and the field's
//! ambient parts.
//!
//! `FUN_8001ADA4` case 4 (`actor[+0x56] == 4`) builds its node's geometry
//! into the scratch block `*(0x8007B85C) + 0x5DC00` and points every slot of
//! the actor's model list `+0x44` at it (`0x8001B08C..0x8001B0B4`); with
//! `+0x9E & 0x4000` the builder is `FUN_8002A5A4` (`jal` at `0x8001B0E8`),
//! one textured quad ([`crate::baka_impact_fx::sprite_arm_quad`]). The case
//! then falls into the ordinary model draw at `0x8001B160`, so the quad is
//! drawn exactly like a mesh part: scaled by `+0x72 / 0x1000` when that is
//! not `0x1000` (`0x8001B240..0x8001B2C4`), turned by the rotation banks,
//! and handed to the prim dispatcher with the colour word `+0x74` and level
//! `+0x78` - the ABE bit and ABR mode ORed into the packet, the packet
//! colour depth-cued toward the word's far colour
//! ([`crate::baka_impact_fx::ColourWord`]).
//!
//! Unlike the `0x2000` ribbon arm, whose builder is battle-overlay code, the
//! sprite arm is SCUS-resident, so it draws in every mode.
//!
//! PORT: FUN_8001ADA4 (case 4's `0x4000` arm, `0x8001B0B8..0x8001B0EC`: the
//! builder call on the node's own `+0x9C` block, then the model-list draw)

use legaia_engine_vm::move_vm::ActorState;

use crate::baka_impact_fx::{ColourWord, sprite_arm_quad};

/// The draw-kind-4 render mode (`actor[+0x56]`) the dispatcher's case 4
/// tests.
pub const DRAW_KIND_MULTI: u16 = 4;

/// `actor[+0x9E]` bit selecting the sprite arm.
pub const SPRITE_ARM_BIT: u16 = 0x4000;

/// Whether a part's state is a live sprite-arm node.
pub fn is_sprite_arm(s: &ActorState) -> bool {
    s.move_substate == DRAW_KIND_MULTI as i16 && s.field_9e & SPRITE_ARM_BIT != 0
}

/// The node's quad as a local-space VRAM mesh (one quad, two triangles),
/// already scaled by `+0x72 / 0x1000`, with the colour word applied to the
/// packet colour and the TSB word. `None` for a state that is not a
/// sprite-arm node or whose plane selector is the uninitialised mode `3`.
pub fn sprite_arm_vram_mesh(s: &ActorState) -> Option<legaia_tmd::mesh::VramMesh> {
    if !is_sprite_arm(s) {
        return None;
    }
    let quad = sprite_arm_quad(s.field_9e, s)?;
    let colour = ColourWord::of(s);
    let rgb = colour.cue(quad.rgb);
    let tsb = {
        let t = quad.tpage | (u16::from(colour.abr) << 5);
        legaia_tmd::mesh::pack_tsb_semi(t, colour.semi)
    };
    let k = f32::from(s.field_72) / 4096.0;
    let mut mesh = legaia_tmd::mesh::VramMesh {
        positions: Vec::with_capacity(4),
        uvs: Vec::with_capacity(4),
        cba_tsb: Vec::with_capacity(4),
        indices: vec![0, 1, 2, 2, 1, 3],
        normals: vec![[0.0; 3]; 4],
        colors: vec![rgb; 4],
    };
    for (v, uv) in quad.verts.iter().zip(quad.uvs) {
        mesh.positions.push(v.map(|c| f32::from(c) * k));
        mesh.uvs.push(uv);
        mesh.cba_tsb.push([quad.clut, tsb]);
    }
    Some(mesh)
}

/// The sprite-arm draw of a part, in the same `(world_pos, rot)` form a
/// ribbon or a mesh part takes ([`crate::effect_ribbon::RibbonDraw`]).
pub fn sprite_arm_draw(s: &ActorState) -> Option<crate::effect_ribbon::RibbonDraw> {
    const A: f32 = std::f32::consts::TAU / 4096.0;
    let mesh = sprite_arm_vram_mesh(s)?;
    Some(crate::effect_ribbon::RibbonDraw {
        mesh,
        world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
        rot: [
            (s.render_24 as f32) * A,
            (s.y_rot.wrapping_add(s.render_26) as f32) * A,
            (s.render_28 as f32) * A,
        ],
        flags_52: s.field_52,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> ActorState {
        let mut s = ActorState {
            move_substate: DRAW_KIND_MULTI as i16,
            field_9e: SPRITE_ARM_BIT, // plane XY
            field_72: 0x2000,
            // ABE on, ABR 1, black far colour, no cue.
            field_74: 0xC100_0000,
            field_78: 0,
            ..Default::default()
        };
        s.set_actor_u16(0xB4, 0x40); // w
        s.set_actor_u16(0xB6, 0x20); // h
        s.set_actor_u16(0xA8, 0x10); // u0
        s.set_actor_u16(0xAA, 0x20); // v0
        s.set_actor_u16(0xAC, 0x2F); // u1
        s.set_actor_u16(0xAE, 0x3F); // v1
        s.set_actor_u16(0xB0, 0x0015); // tpage
        s.set_actor_u16(0xB2, 0x7DC0); // clut
        s.set_actor_u32(0xA0, 0x0080_8080);
        s
    }

    #[test]
    fn a_sprite_arm_node_draws_one_scaled_blended_quad() {
        let m = sprite_arm_vram_mesh(&node()).expect("a sprite-arm node");
        assert_eq!(m.indices, vec![0, 1, 2, 2, 1, 3]);
        // `(∓w/2, ∓h/2)` at +0x72 = 2.0.
        assert_eq!(m.positions[0], [-64.0, -32.0, 0.0]);
        assert_eq!(m.positions[3], [64.0, 32.0, 0.0]);
        assert_eq!(m.uvs[0], [0x10, 0x20]);
        assert_eq!(m.uvs[3], [0x2F, 0x3F]);
        let [cba, tsb] = m.cba_tsb[0];
        assert_eq!(cba, 0x7DC0);
        assert_eq!(tsb & 0x7F, 0x15 | (1 << 5), "ABR 1 ORed into the tpage");
        assert_ne!(
            tsb & legaia_tmd::mesh::TSB_SEMI_TRANSPARENT_BIT,
            0,
            "ABE on"
        );
        // `+0x78` bit 0 is forced on a non-zero mode byte, so the cue moves
        // the colour by less than one step toward black.
        assert_eq!(m.colors[0], [0x7F, 0x7F, 0x7F]);
    }

    #[test]
    fn only_the_0x4000_arm_of_draw_kind_4_is_a_sprite() {
        let mut s = node();
        s.field_9e = 0x2000;
        assert!(sprite_arm_vram_mesh(&s).is_none(), "the ribbon arm");
        s.field_9e = 0;
        assert!(sprite_arm_vram_mesh(&s).is_none(), "the default arm");
        let mut s = node();
        s.move_substate = 5;
        assert!(sprite_arm_vram_mesh(&s).is_none(), "not draw kind 4");
        let mut s = node();
        s.field_9e = SPRITE_ARM_BIT | 3;
        assert!(
            sprite_arm_vram_mesh(&s).is_none(),
            "plane 3 is uninitialised"
        );
    }
}
