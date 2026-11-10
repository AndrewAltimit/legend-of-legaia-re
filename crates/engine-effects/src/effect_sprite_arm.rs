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
//! ([`legaia_render_kernels::sprite_arm::ColourWord`]).
//!
//! Unlike the `0x2000` ribbon arm, whose builder is battle-overlay code, the
//! sprite arm is SCUS-resident, so it draws in every mode.
//!
//! PORT: FUN_8001ADA4 (case 4's `0x4000` arm, `0x8001B0B8..0x8001B0EC`: the
//! builder call on the node's own `+0x9C` block, then the model-list draw)

use legaia_engine_vm::move_vm::ActorState;

use legaia_render_kernels::sprite_arm::{ColourWord, sprite_arm_quad};

/// The draw-kind-4 render mode (`actor[+0x56]`) the dispatcher's case 4
/// tests.
pub const DRAW_KIND_MULTI: u16 = 4;

/// `actor[+0x9E]` bit selecting the sprite arm.
pub const SPRITE_ARM_BIT: u16 = 0x4000;

/// Whether a part's state is a live sprite-arm node.
pub fn is_sprite_arm(s: &ActorState) -> bool {
    s.move_substate == DRAW_KIND_MULTI as i16 && s.field_9e & SPRITE_ARM_BIT != 0
}

/// Whether the prim dispatcher draws the node's quad through handler bank
/// 3's split leaf: the colour word's mode bit `0x20` selects bank 3
/// (`FUN_80043390`), and bank 3 swaps only the gouraud-textured quad leaf
/// (kind 19, `FUN_80045BB4`) - a flat `FT4` takes bank 1's leaf whole.
fn splits_into_gt3s(
    quad: &legaia_render_kernels::sprite_arm::SpriteArmQuad,
    colour: &ColourWord,
) -> bool {
    colour.split_quads && quad.gouraud
}

/// Whether a sprite-arm node's quad draws as bank 3's sixteen `GT3`s, which
/// link on `AVSZ3` (the host's tessellated link offset).
pub fn sprite_arm_tessellated(s: &ActorState) -> bool {
    is_sprite_arm(s)
        && sprite_arm_quad(s.field_9e, s).is_some_and(|q| splits_into_gt3s(&q, &ColourWord::of(s)))
}

/// The node's quad as a local-space VRAM mesh (one quad, two triangles; or
/// sixteen triangles when the colour word routes it to bank 3's split leaf),
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
    // A draw-time packet, not a resident TMD word: the prologue grade's
    // packet rewrite never reaches it (`TSB_RUNTIME_PACKET_BIT`).
    // A `GT4` (or bank 3's `GT3`s) carries one colour on every corner, so
    // it is tagged Gouraud for the dither the shaders cannot read off it
    // (`TSB_GOURAUD_BIT`).
    let tsb = {
        let t = quad.tpage | (u16::from(colour.abr) << 5);
        let gouraud = if quad.gouraud {
            legaia_tmd::mesh::TSB_GOURAUD_BIT
        } else {
            0
        };
        legaia_tmd::mesh::pack_tsb_semi(t, colour.semi)
            | legaia_tmd::mesh::TSB_RUNTIME_PACKET_BIT
            | gouraud
    };
    let k = f32::from(s.field_72) / 4096.0;
    if splits_into_gt3s(&quad, &colour) {
        // Handler bank 3's gouraud-textured quad leaf `FUN_80045BB4`: the
        // quad goes out as sixteen `GT3`s, each cued per vertex.
        use legaia_render_kernels::sprite_arm::{SplitVertex, subdivide_quad};
        let corners = std::array::from_fn(|c| SplitVertex {
            pos: quad.verts[c],
            rgb: quad.rgb,
            uv: quad.uvs[c],
        });
        let tris = subdivide_quad(corners);
        let mut mesh = legaia_tmd::mesh::VramMesh {
            positions: Vec::with_capacity(tris.len() * 3),
            uvs: Vec::with_capacity(tris.len() * 3),
            cba_tsb: Vec::with_capacity(tris.len() * 3),
            indices: Vec::with_capacity(tris.len() * 3),
            normals: vec![[0.0; 3]; tris.len() * 3],
            colors: Vec::with_capacity(tris.len() * 3),
        };
        for v in tris.into_iter().flatten() {
            mesh.indices.push(mesh.positions.len() as u32);
            mesh.positions.push(v.pos.map(|c| f32::from(c) * k));
            mesh.uvs.push(v.uv);
            mesh.cba_tsb.push([quad.clut, tsb]);
            mesh.colors.push(colour.cue(v.rgb));
        }
        return Some(mesh);
    }
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
///
/// The node's matrix is its rotation banks `+0x24 / +0x26 / +0x28` alone
/// (`FUN_80026988` on `actor + 0x24`); `+0x22` is not a rotation here - on a
/// keyframe-pose node it is the blend cursor.
///
/// This is the single-quad draw of a node on the ordinary model path. A node
/// in the keyframe-mesh mode draws one quad per posed part instead
/// ([`sprite_arm_draws`]).
pub fn sprite_arm_draw(s: &ActorState) -> Option<crate::effect_ribbon::RibbonDraw> {
    sprite_arm_draw_row(s, Default::default())
}

/// [`sprite_arm_draw`] under the deformation-table row the part's `+0x6D`
/// names ([`crate::part_shatter::shatter_part_mesh_row`]).
pub fn sprite_arm_draw_row(
    s: &ActorState,
    row: legaia_render_kernels::shatter::ShatterRow,
) -> Option<crate::effect_ribbon::RibbonDraw> {
    let mut mesh = sprite_arm_vram_mesh(s)?;
    // The model draw's `+0x7A` gate ([`crate::part_shatter`]).
    crate::part_shatter::apply_part_shatter_row(
        &mut mesh,
        crate::part_shatter::part_shatter_word(s),
        row,
    );
    Some(crate::effect_ribbon::RibbonDraw {
        mesh,
        world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
        rot: crate::effect_ribbon::node_rotation(s),
        flags_52: s.field_52,
        single_sided: !legaia_render_kernels::sprite_arm::ColourWord::of(s).double_sided,
        tessellated: sprite_arm_tessellated(s)
            && crate::part_shatter::part_shatter_word(s) == 0
            && s.field_42 == 0,
        object_effect_kind: s.field_42,
        object_effect_scale: f32::from(s.field_72) / 4096.0,
    })
}

/// Every quad a sprite-arm node draws.
///
/// Case 4 points **every** slot of the node's model list at the one built
/// quad (`0x8001B08C..0x8001B0B4`), and on falling into the model draw it
/// tests `+0x5A == 6` (`0x8001B160`): a node in the keyframe-mesh mode -
/// seated by move-VM op `0x3C`, which also sets the list's count - is drawn
/// by the animated renderer `FUN_8001B964` with `+0x4C` as its clip, so slot
/// `i` is the quad posed by part `i`'s blended keyframe
/// ([`legaia_engine_vm::move_vm::keyframe_pose_entries`], decoded as
/// `FUN_8001BE80` does): rotated `Rz * Ry * Rx` about its own centre, then
/// translated, all under the node's render scale. `map01`'s mist puffs are
/// this shape - eight sheets per node, spread across the ridge line by the
/// keyframes, where the ordinary path would stack them on the node.
///
/// A keyframe-mesh node without a seated pose block draws nothing (retail's
/// renderer refuses a clip whose part count is not the list's). Any other
/// sprite-arm node is the one quad of [`sprite_arm_draw`].
///
/// PORT: FUN_8001ADA4 (`0x8001B160..0x8001B19C`, the `+0x5A == 6` hand-off
/// to the animated renderer)
/// PORT: FUN_8001B964 (the per-part pose of a sprite-arm node's slots)
pub fn sprite_arm_draws(s: &ActorState) -> Vec<crate::effect_ribbon::RibbonDraw> {
    sprite_arm_draws_row(s, Default::default())
}

/// [`sprite_arm_draws`] under the deformation-table row the part's `+0x6D`
/// names.
pub fn sprite_arm_draws_row(
    s: &ActorState,
    row: legaia_render_kernels::shatter::ShatterRow,
) -> Vec<crate::effect_ribbon::RibbonDraw> {
    let Some(base) = sprite_arm_draw_row(s, row) else {
        return Vec::new();
    };
    if s.move_submode != 6 {
        return vec![base];
    }
    const A: f32 = std::f32::consts::TAU / 4096.0;
    let k = f32::from(s.field_72) / 4096.0;
    legaia_engine_vm::move_vm::keyframe_pose_entries(s)
        .iter()
        .map(|entry| {
            let t = legaia_actor_formats::player_anm::BoneTransform::decode(entry);
            let (sx, cx) = ((t.r_x as f32) * A).sin_cos();
            let (sy, cy) = ((t.r_y as f32) * A).sin_cos();
            let (sz, cz) = ((t.r_z as f32) * A).sin_cos();
            let mut d = base.clone();
            for p in &mut d.mesh.positions {
                let [x, y, z] = *p;
                // Rx, then Ry, then Rz (the product `Rz * Ry * Rx`).
                let (y, z) = (y * cx - z * sx, y * sx + z * cx);
                let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
                let (x, y) = (x * cz - y * sz, x * sz + y * cz);
                *p = [
                    x + t.t_x as f32 * k,
                    y + t.t_y as f32 * k,
                    z + t.t_z as f32 * k,
                ];
            }
            d
        })
        .collect()
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
        assert_ne!(
            tsb & legaia_tmd::mesh::TSB_RUNTIME_PACKET_BIT,
            0,
            "a draw-time packet, outside the prologue grade's TMD-word rewrite"
        );
        // `+0x78` bit 0 is forced on a non-zero mode byte, so the cue moves
        // the colour by less than one step toward black.
        assert_eq!(m.colors[0], [0x7F, 0x7F, 0x7F]);
    }

    /// PROT 0920's storm clouds (`0x801F8244..0x801F8304`): colour word
    /// `0xE9000000`, whose mode bit `0x20` sends the gouraud quad through
    /// bank 3's split leaf as sixteen `GT3`s; a flat quad (`+0x9E` texture
    /// mode non-zero) keeps bank 1's one `FT4`.
    #[test]
    fn a_bank_3_colour_word_splits_the_gouraud_quad_into_sixteen_gt3s() {
        let mut s = node();
        s.field_74 = 0xE900_0000;
        let m = sprite_arm_vram_mesh(&s).expect("a sprite-arm node");
        assert_eq!(m.indices.len(), 16 * 3);
        assert_eq!(m.positions.len(), 16 * 3);
        // The first triangle is centre / centre-corner-0 midpoint / ...
        assert_eq!(m.positions[0], [0.0, 0.0, 0.0]);
        assert!(sprite_arm_draw(&s).unwrap().tessellated);
        // One colour on every corner, still a Gouraud packet: tagged for the
        // dither the shader cannot read off a zero colour derivative.
        assert!(
            m.cba_tsb
                .iter()
                .all(|ct| ct[1] & legaia_tmd::mesh::TSB_GOURAUD_BIT != 0)
        );
        let mut flat = s.clone();
        flat.field_9e |= 1 << 3;
        let m = sprite_arm_vram_mesh(&flat).expect("a sprite-arm node");
        assert_eq!(m.indices, vec![0, 1, 2, 2, 1, 3]);
        assert_eq!(
            m.cba_tsb[0][1] & legaia_tmd::mesh::TSB_GOURAUD_BIT,
            0,
            "an FT4"
        );
        assert!(!sprite_arm_draw(&flat).unwrap().tessellated);
        assert!(!sprite_arm_draw(&node()).unwrap().tessellated);
    }

    #[test]
    fn a_keyframe_pose_node_draws_one_quad_per_posed_part() {
        let mut s = node();
        s.field_72 = 0x1000;
        assert_eq!(sprite_arm_draws(&s).len(), 1, "the ordinary path");
        s.move_submode = 6;
        assert!(
            sprite_arm_draws(&s).is_empty(),
            "mode 6 without a pose block draws nothing"
        );
        // Two parts, X = -256 and +128, no rotation.
        s.keyframe_pose = vec![
            [0, 0, 0, -256, 0, 64, 0, 0, 0, -256, 0, 64],
            [0, 0, 0, 128, 0, 0, 0, 0, 0, 128, 0, 0],
        ];
        let d = sprite_arm_draws(&s);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].mesh.positions[0], [-256.0 - 32.0, -16.0, 64.0]);
        assert_eq!(d[1].mesh.positions[3], [128.0 + 32.0, 16.0, 0.0]);
        // The node's own matrix is its rotation banks; the blend cursor
        // `+0x22` is not a rotation.
        s.y_rot = 0x400;
        assert_eq!(d[0].rot, sprite_arm_draws(&s)[0].rot);
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
