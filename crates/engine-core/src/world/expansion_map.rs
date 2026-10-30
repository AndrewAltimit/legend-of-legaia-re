//! The `World` side of an expansion-pack map (`World::expansion.map`): the
//! encounter zone under the player, which keys the pack's encounter roll, and
//! the map location a save carries.
//!
//! Both are the identity with no map active: the roll keys on the scene
//! label as before, and a save carries no location.

use legaia_engine_expansion::map::MapLocation;

use super::World;

impl World {
    /// The key the pack's encounter tables are looked up under
    /// (`battle.toml` `[[encounters]] scene`): on a pack map the table of
    /// the encounter zone under the player - `None` outside every zone, so
    /// the map rolls nothing there - else the active scene's label.
    pub(in crate::world) fn expansion_encounter_key(&self) -> Option<String> {
        let Some(map) = self.expansion.map.as_ref() else {
            return Some(self.active_scene_label.clone());
        };
        let player = self.actors.get(self.player_actor_slot? as usize)?;
        let tile = map.tile_at_world(
            i32::from(player.move_state.world_x),
            i32::from(player.move_state.world_z),
        )?;
        map.encounter_table_at(tile).map(str::to_string)
    }

    /// The pack map location the player stands at, if on one.
    pub fn expansion_map_location(&self) -> Option<MapLocation> {
        let map = self.expansion.map.as_ref()?;
        let player = self.actors.get(self.player_actor_slot? as usize)?;
        Some(MapLocation {
            map_id: map.map_id.clone(),
            x: player.move_state.world_x,
            z: player.move_state.world_z,
            facing: player.move_state.render_26 as u16 & 0x0FFF,
        })
    }

    /// Stamp the pack map location onto the pack's progress, for the save
    /// about to be written ([`Self::expansion_save_block`]); cleared off every
    /// map. No-op with no pack.
    pub fn expansion_record_map_location(&mut self) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        let location = self.expansion_map_location();
        self.expansion.quests.sync_pack(&pack);
        self.expansion.quests.progress.map = location;
    }

    /// The pack map location the loaded save was made at, if any.
    pub fn expansion_saved_map_location(&self) -> Option<&MapLocation> {
        self.expansion
            .pack
            .as_ref()
            .and(self.expansion.quests.progress.map.as_ref())
    }
}

/// The detail layer's presentation: animated props, lights, mood and
/// animated materials (`docs/formats/expansion-pack-maps.md`, "Detail").
/// All of it reads [`World::frame`] and the map's file, so it is the same on
/// both hosts and in every run, and none of it is simulation state.
impl World {
    /// Each animated prop's live pose on this frame, keyed by its draw-record
    /// key (`legaia_engine_scene::expansion_map::PROP_RECORD_BASE + n`).
    fn expansion_prop_poses(
        &self,
    ) -> impl Iterator<Item = (usize, legaia_engine_expansion::map_detail::Pose)> + '_ {
        let frame = self.frame;
        self.expansion
            .map
            .iter()
            .flat_map(|m| m.def.props.iter().enumerate())
            .filter(|(_, p)| !p.animate.is_empty())
            .map(move |(i, p)| {
                (
                    legaia_engine_scene::expansion_map::PROP_RECORD_BASE + i,
                    legaia_engine_expansion::map_detail::pose(&p.animate, frame),
                )
            })
    }

    /// The animated props' turns, in the form `object_draw_turn_matrices`
    /// hands the hosts.
    pub fn expansion_prop_turns(&self) -> Vec<(usize, [f32; 16])> {
        self.expansion_prop_poses()
            .map(|(r, p)| (r, p.turn_matrix()))
            .collect()
    }

    /// The animated props' moves, in the form `object_draw_displacements`
    /// hands the hosts.
    pub fn expansion_prop_moves(&self) -> Vec<(usize, [i32; 3])> {
        self.expansion_prop_poses()
            .filter(|(_, p)| p.offset != [0.0; 3])
            .map(|(r, p)| (r, p.offset.map(|c| c.round() as i32)))
            .collect()
    }

    /// The map's enhanced-lighting mood, `None` off a map or when the map
    /// takes its base scene's.
    pub fn expansion_map_mood(
        &self,
    ) -> Option<legaia_render_kernels::scene_lighting::LightingMood> {
        use legaia_expansion_format::map_detail::MoodPreset;
        use legaia_render_kernels::scene_lighting::LightingMood;
        Some(match self.expansion.map.as_ref()?.def.lighting.mood? {
            MoodPreset::Day => LightingMood::DAY,
            MoodPreset::Dusk => LightingMood::DUSK,
            MoodPreset::Night => LightingMood::NIGHT,
            MoodPreset::Cave => LightingMood::CAVE,
        })
    }

    /// The map's placed lamps and prop lights on this frame, flicker
    /// applied, in the retail field frame the scene's derived lights use.
    pub fn expansion_map_lights(
        &self,
    ) -> Vec<legaia_render_kernels::scene_lighting::ScenePointLight> {
        use legaia_engine_expansion::map_detail::{flicker_gain, pose};
        let Some(map) = self.expansion.map.as_ref() else {
            return Vec::new();
        };
        let def = &map.def;
        let light = |seed: u32,
                     tile: [u16; 2],
                     offset: [f32; 3],
                     color: [u8; 3],
                     radius: u16,
                     flicker: f32,
                     period: i32| {
            let (cx, cz) = def.tile_centre(tile[0], tile[1]);
            let (x, z) = (cx as f32 + offset[0], cz as f32 + offset[2]);
            let floor = self.sample_field_floor_height(x as i32, z as i32) as f32;
            let g = flicker_gain(flicker, period, seed, self.frame);
            legaia_render_kernels::scene_lighting::ScenePointLight {
                pos: [x, floor - offset[1], z],
                color: color.map(|c| f32::from(c) / 255.0 * g),
                radius: f32::from(radius),
            }
        };
        let mut out: Vec<_> = def
            .lighting
            .lamps
            .iter()
            .enumerate()
            .map(|(i, l)| {
                light(
                    i as u32,
                    l.tile,
                    l.offset.map(f32::from),
                    l.color,
                    l.radius,
                    l.flicker,
                    l.period,
                )
            })
            .collect();
        for (i, p) in def.props.iter().enumerate() {
            let Some(l) = &p.light else {
                continue;
            };
            // A light rides its prop's animated move (a bobbing lantern).
            let moved = pose(&p.animate, self.frame).offset;
            let o = p.offset.map(f32::from);
            out.push(light(
                1000 + i as u32,
                p.tile,
                [
                    o[0] + moved[0],
                    o[1] + f32::from(l.height) - moved[1],
                    o[2] + moved[2],
                ],
                l.color,
                l.radius,
                l.flicker,
                l.period,
            ));
        }
        out
    }

    /// Apply the map's animated materials to `vram` for this frame. Returns
    /// `true` when a word changed (the host re-uploads). No-op off a map.
    pub fn step_expansion_materials(&mut self, vram: &mut legaia_tim::Vram) -> bool {
        use legaia_expansion_format::map_detail::MaterialFx;
        let frame = self.frame;
        let Some(map) = self.expansion.map.as_mut() else {
            return false;
        };
        let def = map.def.clone();
        let mut wrote = false;
        for (i, fx) in def.materials.iter().enumerate() {
            let Some((rect, texels)) = material_rect(fx) else {
                continue;
            };
            let src = map
                .material_captures
                .entry(i)
                .or_insert_with(|| super::ambient::read_rect(vram, rect.0, rect.1, rect.2, rect.3))
                .clone();
            let step = frame / fx.period().max(1) as u64;
            let out = match *fx {
                MaterialFx::Cycle { count, reverse, .. } => {
                    legaia_engine_expansion::map_detail::cycle_palette(
                        &src,
                        0,
                        usize::from(count),
                        step,
                        reverse,
                    )
                }
                MaterialFx::Hsv {
                    period, value, hue, ..
                } => {
                    let t = (frame % period.max(1) as u64) as f32 / period.max(1) as f32;
                    let tau = std::f32::consts::TAU;
                    legaia_engine_field::clut_cell_fx::apply_hsv_cell(
                        &src,
                        &legaia_engine_field::clut_cell_fx::ClutCellFx {
                            rect,
                            h_add: (hue * t).round() as i16,
                            s_add: 0,
                            v_add: (f32::from(value) * (t * tau).sin()).round() as i16,
                            mode: 0,
                            white: 0,
                        },
                    )
                }
                MaterialFx::Scroll { dx, dy, .. } => {
                    let Some(tpw) = texels else { continue };
                    scroll_texels(&src, rect.2, rect.3, tpw, step, dx, dy)
                }
            };
            let cur = super::ambient::read_rect(vram, rect.0, rect.1, rect.2, rect.3);
            if cur != out {
                super::ambient::write_rect(vram, rect.0, rect.1, rect.2, &out);
                wrote = true;
            }
        }
        wrote
    }
}

/// A VRAM rect `(x, y, w, h)` in halfwords.
type VramRect = (u16, u16, u16, u16);

/// The VRAM rect `(x, y, w, h)` a material animates, and for a texel scroll
/// the texels a halfword holds.
fn material_rect(
    fx: &legaia_expansion_format::map_detail::MaterialFx,
) -> Option<(VramRect, Option<u16>)> {
    use legaia_expansion_format::map_detail::MaterialFx;
    let clut_xy = |cba: u16| ((cba & 0x3F) * 16, (cba >> 6) & 0x1FF);
    match *fx {
        MaterialFx::Cycle {
            clut, start, count, ..
        }
        | MaterialFx::Hsv {
            clut, start, count, ..
        } => {
            let (x, y) = clut_xy(clut);
            let x = x + u16::from(start);
            let w = count.min(1024u16.saturating_sub(x));
            (w > 0).then_some(((x, y, w, 1), None))
        }
        MaterialFx::Scroll { cell, rect, .. } => {
            let tsb = u16::from(cell.tpage);
            let (px, py) = ((tsb & 0xF) * 64, ((tsb >> 4) & 1) * 256);
            let tpw: u16 = match (tsb >> 7) & 3 {
                0 => 4,
                1 => 2,
                _ => 1,
            };
            // An explicit rectangle (a carried-in texture's copy) stands in
            // for the cell's.
            if let Some([x, y, w, h]) = rect {
                return (w > 0 && h > 0).then_some(((x, y, w, h), Some(tpw)));
            }
            let axis = legaia_scene_formats::field_objects::GROUND_ATLAS_AXIS as u16;
            let px_cell = legaia_scene_formats::field_objects::GROUND_ATLAS_TILE_PX as u16;
            let (col, row) = (
                u16::from(cell.tile) % axis,
                u16::from(cell.tile) / axis % axis,
            );
            Some((
                (
                    px + col * px_cell / tpw,
                    py + row * px_cell,
                    px_cell / tpw,
                    px_cell,
                ),
                Some(tpw),
            ))
        }
    }
}

/// Rotate a `w x h`-halfword rect's texels (`tpw` a halfword) by
/// `step * (dx, dy)` texels, wrapping.
fn scroll_texels(src: &[u16], w: u16, h: u16, tpw: u16, step: u64, dx: i8, dy: i8) -> Vec<u16> {
    let (w, h, tpw) = (usize::from(w), usize::from(h), usize::from(tpw));
    let tw = w * tpw;
    let bits = 16 / tpw;
    let mask = if bits == 16 {
        0xFFFF
    } else {
        (1u16 << bits) - 1
    };
    let texel = |x: usize, y: usize| (src[y * w + x / tpw] >> ((x % tpw) * bits)) & mask;
    let sx = (step as i64 * i64::from(dx)).rem_euclid(tw as i64) as usize;
    let sy = (step as i64 * i64::from(dy)).rem_euclid(h as i64) as usize;
    let mut out = vec![0u16; src.len()];
    for y in 0..h {
        for x in 0..tw {
            let t = texel((x + tw - sx) % tw, (y + h - sy) % h);
            out[y * w + x / tpw] |= t << ((x % tpw) * bits);
        }
    }
    out
}

impl World {
    /// This frame's pack-map particles ([`legaia_expansion_format::map_detail::EmitterDef`])
    /// as camera-facing sheets through `view`, in the form the fog pool's
    /// sheets take, so both hosts draw them through the same field
    /// screen-effect pass. Empty off a map, outside the field, or for a map
    /// with no emitters. Each particle is a pure function of the frame
    /// (`Emitter::particle`).
    pub fn expansion_particle_quads(
        &self,
        view: &legaia_engine_field::fog_particles::FogView,
    ) -> Vec<legaia_engine_field::fog_particles::FogQuad> {
        let Some(map) = self.expansion.map.as_ref() else {
            return Vec::new();
        };
        let Some(sheet) = map.particle_sheet else {
            return Vec::new();
        };
        if self.mode != legaia_engine_mode::mode::SceneMode::Field {
            return Vec::new();
        }
        let def = &map.def;
        let centre = |c: u16, r: u16| def.tile_centre(c, r);
        let mut out = Vec::new();
        for (e, emitter) in def.emitters.iter().enumerate() {
            let em = emitter.resolved();
            // ABR 1 (additive) for the glowing kinds; a solid one (leaves)
            // draws through the STP-less ramp, opaque.
            let abr: u16 = u16::from(em.additive);
            let clut = if em.additive {
                sheet.clut_glow
            } else {
                sheet.clut_solid
            };
            let tpage = sheet.tpage | (abr << 5);
            let su = sheet.u0 + em.kind.sprite() * 16;
            for i in 0..em.count {
                let Some(p) = em.particle(e, i, self.frame, centre) else {
                    continue;
                };
                if p.color == [0, 0, 0] {
                    continue;
                }
                let (x, z) = (p.pos[0] as i32, p.pos[2] as i32);
                let floor = self.sample_field_floor_height(x, z);
                let world = [x, floor - p.pos[1] as i32, z];
                let Some((sx, sy, sz)) = view.project(world) else {
                    continue;
                };
                // The sprite's screen half-size: a world-space offset of its
                // size, projected.
                let half = (p.size / 2.0).max(1.0) as i32;
                let side = view.project([world[0] + half, world[1], world[2]]);
                let up = view.project([world[0], world[1] - half, world[2]]);
                let r = [side, up]
                    .iter()
                    .flatten()
                    .map(|&(ax, ay, _)| (ax - sx).abs().max((ay - sy).abs()))
                    .max()
                    .unwrap_or(1)
                    .clamp(1, 96);
                if sx + r < -16 || sx - r > 336 || sy + r < -16 || sy - r > 256 {
                    continue;
                }
                let px = |v: i32| v.clamp(-1024, 1023) as i16;
                out.push(legaia_engine_field::fog_particles::FogQuad {
                    xy: [
                        (px(sx - r), px(sy - r)),
                        (px(sx + r), px(sy - r)),
                        (px(sx - r), px(sy + r)),
                        (px(sx + r), px(sy + r)),
                    ],
                    uv: [
                        (su, sheet.v0),
                        (su + 15, sheet.v0),
                        (su, sheet.v0 + 15),
                        (su + 15, sheet.v0 + 15),
                    ],
                    clut,
                    tpage,
                    rgb: p.color,
                    ot_index: (sz.max(0) as u32) >> 5,
                    sz: sz as f32,
                    depth: view.ndc_depth(world),
                });
            }
        }
        out
    }

    /// Arm the retail field fog pool over the active map's `[fog]` regions:
    /// the regions a scene's MAN section 4 would install, and the ambient
    /// particle gate a scene's script would raise. No-op off a map or for a
    /// map with no fog.
    pub fn arm_expansion_fog(&mut self) {
        let Some(map) = self.expansion.map.as_ref() else {
            return;
        };
        let def = map.def.clone();
        if def.fog.regions.is_empty() {
            return;
        }
        let [ox, oz] = def.tiles.origin.map(i32::from);
        let sixteenths = |deg: f32| ((deg / 22.5).round() as i32).rem_euclid(256) as u8;
        self.fog.regions = def
            .fog
            .regions
            .iter()
            .map(|r| {
                // The record's bounds are open: `x0 < tile < x1`.
                let b = |v: i32| v.clamp(0, 127) as u8;
                let (c0, c1) = (r.rect[0].min(r.rect[2]), r.rect[0].max(r.rect[2]));
                let (r0, r1) = (r.rect[1].min(r.rect[3]), r.rect[1].max(r.rect[3]));
                legaia_render_kernels::fog_volume::FogRegion {
                    enabled: true,
                    x0: b(ox + i32::from(c0) - 1),
                    z0: b(oz + i32::from(r0) - 1),
                    x1: b(ox + i32::from(c1) + 1),
                    z1: b(oz + i32::from(r1) + 1),
                    angle_base: sixteenths(r.angle - r.spread),
                    angle_spread: sixteenths(2.0 * r.spread),
                    speed: r.speed,
                    byte_8: 0,
                    flag_index: 0,
                }
            })
            .collect();
        self.set_ambient_particles_enabled(true);
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;

    #[test]
    fn a_texel_scroll_wraps_by_texel_not_by_word() {
        // One 4bpp row of 8 texels: 0..8.
        let src = [0x3210u16, 0x7654];
        let out = scroll_texels(&src, 2, 1, 4, 1, 1, 0);
        assert_eq!(out, vec![0x2107, 0x6543]);
        assert_eq!(scroll_texels(&src, 2, 1, 4, 8, 1, 0), src.to_vec());
    }

    #[test]
    fn a_scroll_cell_is_its_atlas_rect() {
        use legaia_expansion_format::map_detail::{AtlasCell, MaterialFx};
        let fx = MaterialFx::Scroll {
            cell: AtlasCell {
                tpage: 28,
                clut: 0,
                tile: 9,
            },
            dx: 1,
            dy: 0,
            period: 2,
            from: None,
            rect: None,
        };
        // Page 28 = (768, 256), 4bpp; tile 9 = column 1, row 1 of the atlas.
        assert_eq!(
            material_rect(&fx),
            Some(((768 + 8, 256 + 32, 8, 32), Some(4)))
        );
        // A resolved copy's rectangle stands in for the cell's.
        let MaterialFx::Scroll {
            cell,
            dx,
            dy,
            period,
            ..
        } = fx
        else {
            unreachable!()
        };
        let moved = MaterialFx::Scroll {
            cell,
            dx,
            dy,
            period,
            from: Some("rugi".into()),
            rect: Some([100, 200, 8, 32]),
        };
        assert_eq!(material_rect(&moved), Some(((100, 200, 8, 32), Some(4))));
    }
}
