//! A monster's portrait: its battle mesh in the idle action's first pose,
//! rendered small through [`crate::mesh_raster`] - the art a trading card
//! shows.
//!
//! The mesh and texture are staged the way the battle loader stages slot 0
//! ([`crate::monster_archive::MonsterMesh::battle_render_mesh`], plus the boss
//! effect page for the boss ids that sample it), so the picture is the
//! monster as a fight draws it, read off the player's own disc. The result is
//! an in-memory RGBA image; nothing here writes one anywhere.

use crate::mesh_raster::{self, Pose, RasterOptions};
use crate::monster_archive;

/// The framing a card portrait uses: a three-quarter view from slightly
/// above, the monster turned to face the viewer.
pub fn portrait_options(width: usize, height: usize) -> RasterOptions {
    RasterOptions {
        width,
        height,
        yaw: 200f32.to_radians(),
        pitch: -12f32.to_radians(),
        margin: 0.04,
        background: [0, 0, 0, 0],
        shade: 0.35,
        auto_orient: false,
    }
}

/// Render monster `id` (1-based) of the monster archive `archive` (PROT 867,
/// extended footprint) at `width x height`. `None` for an empty slot or a
/// mesh that draws nothing.
pub fn render(archive: &[u8], id: u16, opts: &RasterOptions) -> Option<Vec<u8>> {
    let monster = monster_archive::mesh(archive, id).ok()??;
    let tmd = legaia_tmd::parse(monster.tmd_bytes()).ok()?;
    let (_, object_ids) =
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, monster.tmd_bytes());
    let mut vram = legaia_tim::Vram::new();
    let mesh = monster.battle_render_mesh(0, &mut vram)?;
    monster_archive::write_boss_effect_page(archive, id, &mut vram);
    if mesh.positions.is_empty() || mesh.indices.is_empty() {
        return None;
    }
    // The idle action's first frame poses the parts; without one they sit
    // at the origin as stored.
    let poses: Vec<Pose> = monster_archive::animations(archive, id)
        .ok()
        .flatten()
        .and_then(|a| {
            a.into_iter()
                .find(|a| a.part_count > 0 && a.frame_count > 0)
        })
        .and_then(|a| a.frame(0).map(<[_]>::to_vec))
        .map(|f| {
            f.iter()
                .map(|p| Pose::from_keyframe([p.tx, p.ty, p.tz], [p.rx, p.ry, p.rz]))
                .collect()
        })
        .unwrap_or_default();
    let img = mesh_raster::render_posed(&mesh, &object_ids, &poses, &vram, opts);
    (!img.is_empty() && img.as_chunks::<4>().0.iter().any(|p| p[3] != 0)).then_some(img)
}
