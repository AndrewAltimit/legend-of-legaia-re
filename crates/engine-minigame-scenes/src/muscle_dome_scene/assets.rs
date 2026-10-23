//! The loaded arena assets: fighters, monsters, arena mesh and VRAM, and the pose kernel.

use super::*;
use legaia_actor_formats::battle_char_assembly as bca;
use legaia_actor_formats::monster_archive;
use legaia_engine_battle_menus::muscle_dome_session::choreo;
use legaia_scene_formats::scene_tmd_stream;
use legaia_tmd::mesh::VramMesh;
use std::sync::Arc;

/// The decoded, seated dome: the static buffers and the VRAM for one
/// `(monster, character)` pair.
pub struct MuscleDomeAssets {
    pub(super) key: (u16, u32),
    pub(super) bodies: [DomeBody; 2],
    /// Everything that never moves (arena shell + ground grid, or the
    /// fallback floor), appended after the two bodies.
    pub(super) statics: VramMesh,
    pub(super) statics_flat: Vec<u8>,
    vram: legaia_tim::Vram,
    arena: bool,
    /// The monster record's size class (`+0x1F`), which sizes the action
    /// framing's depth (`FUN_801F0348`).
    pub(super) size_class: u8,
    /// The per-character close-up heights (`0x801F4D2C`).
    pub(super) heights: Option<legaia_game_tables::battle_camera_table::BattleCameraHeights>,
}

impl std::fmt::Debug for MuscleDomeAssets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuscleDomeAssets")
            .field("key", &self.key)
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

fn fighter_build(raw: &[u8]) -> Option<(bca::AssembledCharacter, VramMesh, Vec<u32>)> {
    let pack = legaia_actor_formats::battle_data_pack::parse(raw).ok()?;
    let mut asm = bca::assemble_character(raw, &pack, &[0u8; 5]).ok()?;
    bca::relocate_tsb_cba(&mut asm.tmd, 0).ok()?;
    let tmd = legaia_tmd::parse(&asm.tmd).ok()?;
    let (mesh, oids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
    (!mesh.indices.is_empty()).then_some((asm, mesh, oids))
}

impl MuscleDomeAssets {
    /// Decode the dome for `(monster_id, char_slot)`. `None` when the
    /// character's battle form does not assemble or the monster's mesh or
    /// idle does not decode on this image.
    pub fn load(
        read_prot: &impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        monster_id: u16,
        char_slot: u32,
    ) -> Option<Self> {
        let player_file = read_prot(
            legaia_actor_formats::battle_texture_catalog::PLAYER_FILE_ENTRIES[0] as usize
                + char_slot.min(2) as usize,
        )?;
        let raw = player_file.as_slice();
        let (asm, fmesh, foids) = fighter_build(raw)?;
        let fighter = DomeBody {
            mesh: fmesh,
            oids: foids,
            idle: choreo::fighter_clip(raw, &asm, 0).and_then(|a| DomeClip::from_anim(&a))?,
        };

        let archive = read_prot(MONSTER_ARCHIVE_PROT_INDEX)?;
        let mut vram = legaia_tim::Vram::new();
        // The character's band-0 texture pool + battle palette.
        if let Ok(pack) = legaia_actor_formats::battle_data_pack::parse(raw) {
            if let Ok(uploads) = bca::character_texture_uploads(raw, &pack, &[0u8; 5], 0) {
                for u in &uploads {
                    vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                    if !u.clut.is_empty() {
                        vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                    }
                }
            }
            let mut rows: Vec<u16> = fighter
                .mesh
                .cba_tsb
                .iter()
                .map(|c| (c[0] >> 6) & 0x1FF)
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let mut cols: Vec<u16> = fighter
                .mesh
                .cba_tsb
                .iter()
                .map(|c| (c[0] & 0x3F) * 16)
                .collect();
            cols.sort_unstable();
            cols.dedup();
            // Vahn = the byte-exact fixed-stride record parse; the others =
            // the equipment-robust collector.
            let pal = if char_slot == 0 {
                legaia_actor_formats::battle_char_palette::find_record0(raw).and_then(|rec0| {
                    legaia_actor_formats::battle_char_palette::parse_record(raw, rec0).ok()
                })
            } else {
                legaia_actor_formats::battle_char_palette::collect_palette(raw, 0, &cols).ok()
            };
            if let Some(pal) = pal {
                for &row in &rows {
                    for band in &pal.bands {
                        let bytes: Vec<u8> = band
                            .vram_words()
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect();
                        vram.write_clut_row(band.base, row, &bytes);
                    }
                }
            }
        }
        let monster_mesh = monster_archive::mesh(&archive, monster_id).ok()??;
        // Injects the monster's pool at slot 0's CLUT row + page origin.
        let mmesh = monster_mesh.battle_render_mesh(0, &mut vram)?;
        let mtmd = legaia_tmd::parse(monster_mesh.tmd_bytes()).ok()?;
        let moids =
            legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&mtmd, monster_mesh.tmd_bytes()).1;
        // Entry 0 is the idle loop.
        let m_idle = monster_archive::idle_animation(&archive, monster_id)
            .ok()
            .flatten()
            .and_then(|a| DomeClip::from_anim(&a))?;
        let size_class = monster_archive::record(&archive, monster_id)
            .ok()
            .flatten()
            .map_or(0, |r| r.size_class);
        let monster = DomeBody {
            mesh: mmesh,
            oids: moids,
            idle: m_idle,
        };

        let mut statics = empty_mesh();
        let mut statics_flat = Vec::new();
        let arena_buf = read_prot(ARENA_BACKDROP_PROT_INDEX);
        let arena = arena_buf.as_deref().and_then(|b| arena_ring(b));
        if let Some(buf) = arena_buf.as_deref()
            && arena.is_some()
        {
            for chunk in scene_tmd_stream::battle_tim_chunks(buf) {
                if let Some(bytes) = buf.get(chunk.payload_offset..)
                    && let Ok(tim) = legaia_tim::parse(bytes)
                {
                    vram.upload_tim(&tim);
                }
            }
        }
        let has_arena = arena.is_some();
        if let Some((m, flat)) = arena {
            append(&mut statics, &mut statics_flat, &m, &flat);
            ground_grid(&mut statics, &mut statics_flat);
        }
        let heights = read_prot(BATTLE_ACTION_PROT_INDEX)
            .and_then(|raw| legaia_game_tables::battle_camera_table::parse(&raw));
        let mut assets = Self {
            key: (monster_id, char_slot),
            bodies: [fighter, monster],
            statics,
            statics_flat,
            vram,
            arena: has_arena,
            size_class,
            heights,
        };
        if !has_arena {
            let reach = assets.reach();
            checker_floor(&mut assets.statics, &mut assets.statics_flat, reach);
        }
        Some(assets)
    }

    /// The seated `(monster_id, char_slot)`.
    pub fn key(&self) -> (u16, u32) {
        self.key
    }

    /// The dome VRAM (character pool + palette, monster pool, arena pages).
    pub fn vram(&self) -> &legaia_tim::Vram {
        &self.vram
    }

    /// Rest-pose half-width and height of body `i`.
    fn extent(&self, i: usize) -> (f32, f32) {
        let b = &self.bodies[i];
        let mut out = b.mesh.positions.clone();
        pose_into(
            &mut out,
            &b.mesh.positions,
            &b.oids,
            &b.idle.frames[0],
            [0.0; 3],
        );
        let (mut lo, mut hi, mut top) = (f32::INFINITY, f32::NEG_INFINITY, 0.0f32);
        for p in &out {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
            top = top.max(-p[1]);
        }
        let half = (hi - lo) / 2.0;
        (
            if half.is_finite() && half != 0.0 {
                half
            } else {
                200.0
            },
            if top != 0.0 { top } else { 400.0 },
        )
    }

    /// The fallback floor's half-extent: both rest half-widths with a
    /// margin.
    pub(super) fn reach(&self) -> f32 {
        let (hp, _) = self.extent(0);
        let (hm, _) = self.extent(1);
        (hp + hm) * 1.5 + 120.0
    }
}

/// Pose `base` into `out` from one keyframe (per part `[tx, ty, tz, rx, ry,
/// rz]`): per object `Rz Ry Rx v + T`, then the world yaw about Y and the
/// `(dx, dz)` floor offset (`place = [dx, yaw, dz]`).
pub(super) fn pose_into(
    out: &mut [[f32; 3]],
    base: &[[f32; 3]],
    oids: &[u32],
    keys: &[[i32; 6]],
    place: [f32; 3],
) {
    let a2r = std::f32::consts::TAU / 4096.0;
    let rots: Vec<([f32; 3], [f32; 3], [f32; 3])> = keys
        .iter()
        .map(|k| {
            let (sx, cx) = (k[3] as f32 * a2r).sin_cos();
            let (sy, cy) = (k[4] as f32 * a2r).sin_cos();
            let (sz, cz) = (k[5] as f32 * a2r).sin_cos();
            (
                [sx, sy, sz],
                [cx, cy, cz],
                [k[0] as f32, k[1] as f32, k[2] as f32],
            )
        })
        .collect();
    let [dx, yaw, dz] = place;
    let (ws, wc) = yaw.sin_cos();
    for (v, (o, b)) in out.iter_mut().zip(base.iter()).enumerate() {
        let [mut x, mut y, mut z] = *b;
        if let Some((s, c, t)) = oids.get(v).and_then(|&o| rots.get(o as usize)) {
            let (ny, nz) = (y * c[0] - z * s[0], y * s[0] + z * c[0]);
            y = ny;
            z = nz;
            let (nx, nz) = (x * c[1] + z * s[1], -x * s[1] + z * c[1]);
            x = nx;
            z = nz;
            let (nx, ny) = (x * c[2] - y * s[2], x * s[2] + y * c[2]);
            x = nx + t[0];
            y = ny + t[1];
            z += t[2];
        }
        *o = [x * wc + z * ws + dx, y, -x * ws + z * wc + dz];
    }
}
