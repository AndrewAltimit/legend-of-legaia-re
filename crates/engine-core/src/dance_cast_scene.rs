//! The dance floor's **bodies as a 3D surface**: every actor the spawner puts
//! on the floor ([`crate::dance::DanceGame::body_frames`]) as one combined,
//! posed vertex buffer in raw retail world coordinates - one kernel every
//! host draws, the dance twin of [`crate::baka_duel_scene`].
//!
//! * **Who stands where** is the rules engine's: the mode's spawn table, the
//!   how-to mode's Disco King, each body's display clip track. A host never
//!   picks a cast, which is what keeps a finals or free-play floor from
//!   drawing the qualifier's three.
//! * **Which mesh** is the kind descriptor's model id: kind 0's indexes the
//!   resident global pool (Noa's field mesh, PROT 0874 §0 slot 1, capped to
//!   the ten live groups `FUN_8001E890` draws), every other one the
//!   dance-hall scene's TMD pool ([`crate::dance_venue::DanceVenue`]).
//! * **The pose** is the clip driver's: the track's ticks turned into the
//!   1/16-frame cursor by [`crate::field_anim::clip_step`] on the record's
//!   own gate + divisor, the loop wrapping at the end tick
//!   ([`crate::field_anim::clip_end_ticks`]), a move held on its last frame
//!   until it has run its length, and the record's sub-frame blend
//!   ([`legaia_asset::player_anm::blend_bone_transform`]). Each object is
//!   `Rz.Ry.Rx . v + T`, then the actor's render scale (`+0x72`), its yaw
//!   about Y and its position.
//!
//! The buffers' textures sample the venue's VRAM (`DanceVenue::resources`),
//! which already carries Noa's field atlas; a host draws them with the same
//! view-projection it frames the hall with.
//!
//! What stays outside: the translucent draw some move clips ask for (anim
//! word bit `0x200`), which no host's dancer pass applies yet.

use std::ops::Range;
use std::sync::Arc;

use crate::dance::{DanceBodyClip, DanceBodyFrame, DanceBodyModel, DanceGame};
use crate::dance_venue::DanceVenue;
use crate::field_anim::{clip_end_ticks, clip_step};
use legaia_asset::player_anm::{BoneTransform, PlayerAnmBundle, blend_bone_transform};

/// Live TMD groups of an active-party field mesh: groups 10 / 11 are the
/// equipment templates, never drawn (`FUN_8001E890`).
pub const ACTIVE_PARTY_LIVE_GROUPS: u32 = 10;

/// Noa's field mesh out of the character pack (PROT 0874) - resident global
/// pool slot 1, the model kind 0's descriptor names - with an active-party
/// mesh's object count capped to [`ACTIVE_PARTY_LIVE_GROUPS`].
pub fn resident_body_tmd(character_pack: &[u8], slot: usize) -> Option<Vec<u8>> {
    let pack = legaia_asset::character_pack::parse(character_pack).ok()?;
    let cslot = pack.slot(slot)?;
    let mut tmd_bytes = cslot.tmd_bytes.clone();
    if cslot.is_active_party() && tmd_bytes.len() >= 0x0C {
        tmd_bytes[0x08..0x0C].copy_from_slice(&ACTIVE_PARTY_LIVE_GROUPS.to_le_bytes());
    }
    Some(tmd_bytes)
}

/// One loadable body mesh.
#[derive(Debug, Clone)]
struct BodyMesh {
    tmd: legaia_tmd::Tmd,
    raw: Vec<u8>,
}

impl BodyMesh {
    fn parse(raw: Vec<u8>) -> Option<Self> {
        let tmd = legaia_tmd::parse(&raw).ok()?;
        Some(Self { tmd, raw })
    }
}

/// Everything the bodies draw from: the meshes each model id can name, and
/// the venue's choreography bank. Built once per venue.
#[derive(Debug, Clone)]
pub struct DanceCastAssets {
    resident: Vec<(u16, BodyMesh)>,
    scene: Vec<(u16, BodyMesh)>,
    anm: PlayerAnmBundle,
}

impl DanceCastAssets {
    /// Collect the meshes off a built `venue` for every model the dance
    /// overlay's descriptors (`cast`) and the how-to spawner can name, plus
    /// kind 0's resident mesh out of `character_pack` (PROT 0874's bytes).
    /// `None` without the venue's ANM bundle - no clip could pose.
    pub fn from_venue(
        venue: &DanceVenue,
        cast: &legaia_asset::dance_cast::DanceCast,
        character_pack: Option<&[u8]>,
    ) -> Option<Self> {
        let anm = venue.anm.clone()?;
        let mut resident = Vec::new();
        let mut scene: Vec<(u16, BodyMesh)> = Vec::new();
        for (kind, k) in cast.kinds.iter().enumerate() {
            if kind == 0 {
                if let Some(m) = character_pack
                    .and_then(|p| resident_body_tmd(p, usize::from(k.model)))
                    .and_then(BodyMesh::parse)
                {
                    resident.push((k.model, m));
                }
                continue;
            }
            Self::push_scene(&mut scene, venue, k.model);
        }
        Self::push_scene(&mut scene, venue, crate::dance::DEMO_MODEL);
        Some(Self {
            resident,
            scene,
            anm,
        })
    }

    fn push_scene(scene: &mut Vec<(u16, BodyMesh)>, venue: &DanceVenue, model: u16) {
        if scene.iter().any(|(m, _)| *m == model) {
            return;
        }
        if let Some(t) = venue.resources.tmds.get(usize::from(model)) {
            scene.push((
                model,
                BodyMesh {
                    tmd: t.tmd.clone(),
                    raw: t.raw.clone(),
                },
            ));
        }
    }

    fn mesh(&self, model: DanceBodyModel) -> Option<&BodyMesh> {
        let (pool, id) = match model {
            DanceBodyModel::Resident(id) => (&self.resident, id),
            DanceBodyModel::Scene(id) => (&self.scene, id),
        };
        pool.iter().find(|(m, _)| *m == id).map(|(_, b)| b)
    }

    /// The choreography bank.
    pub fn anm(&self) -> &PlayerAnmBundle {
        &self.anm
    }
}

/// What a body's display track resolves to this tick: the record, the
/// 1/16-frame cursor, and whether the last frame holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipCursor {
    /// ANM bundle record.
    pub record: usize,
    /// The clip driver's cursor `+0x68`.
    pub cursor: u32,
    /// The clip holds its last frame (a move) instead of wrapping.
    pub hold: bool,
}

/// The step and end tick of one clip on `bank`: `(frames, step, end_ticks)`.
fn clip_timing(bank: &PlayerAnmBundle, clip: DanceBodyClip) -> Option<(u16, u16, u32)> {
    let record = clip.record()?;
    let rec = bank.record_lenient(record).ok()?;
    let step = clip_step(clip.rate, rec.blends(), (rec.flag & 0xFF) as u8);
    Some((rec.frame_count, step, clip_end_ticks(rec.frame_count, step)))
}

/// Resolve a body's track to the clip and cursor it shows.
///
/// A move plays from its first frame and holds its last until its end tick;
/// from then on the standing loop plays from its own first frame, wrapping
/// every [`clip_end_ticks`] - `FUN_801d1358` rebinding the loop off the clip
/// driver's end flag.
pub fn resolve_clip(bank: &PlayerAnmBundle, body: &DanceBodyFrame) -> Option<ClipCursor> {
    let mut ticks = body.ticks;
    if let Some(mv) = body.move_clip
        && let Some((frames, step, end)) = clip_timing(bank, mv)
    {
        if ticks < end.max(1) {
            let last = u32::from(frames) * 16 - 1;
            return Some(ClipCursor {
                record: mv.record()?,
                cursor: (ticks * u32::from(step)).min(last),
                hold: true,
            });
        }
        ticks -= end.max(1);
    }
    let (_, step, end) = clip_timing(bank, body.loop_clip)?;
    Some(ClipCursor {
        record: body.loop_clip.record()?,
        cursor: (ticks % end.max(1)) * u32::from(step),
        hold: false,
    })
}

/// Bone `bone` of `c`: the frame the cursor names, blended toward the next
/// one on the record's gate (`FUN_8001BE80`'s two-frame sampler, over the
/// lenient header - several choreography records carry frame data past their
/// count, which the cursor never reaches).
fn sample(bank: &PlayerAnmBundle, c: ClipCursor, bone: usize) -> Option<BoneTransform> {
    let rec = bank.record_lenient(c.record).ok()?;
    let frames = usize::from(rec.frame_count);
    if frames == 0 {
        return None;
    }
    let frame = ((c.cursor >> 4) as usize).min(frames - 1);
    let cur = bank.bone_transform(c.record, frame, bone)?;
    let frac = (c.cursor & 0xF) as i32;
    if !rec.blends() || frac == 0 {
        return Some(cur);
    }
    let next_frame = if frame < frames - 1 {
        frame + 1
    } else if c.hold {
        frame
    } else {
        0
    };
    let next = bank.bone_transform(c.record, next_frame, bone)?;
    Some(blend_bone_transform(cur, next, frac))
}

fn angle(a: i32) -> f32 {
    a as f32 / 4096.0 * std::f32::consts::TAU
}

/// The floor's bodies as combined buffers. Positions are raw retail world
/// coordinates (Y down) at 1x; every other attribute is static for the
/// scene's life.
#[derive(Debug, Clone)]
pub struct DanceCastScene {
    models: Vec<DanceBodyModel>,
    /// Posed positions, rewritten by [`Self::pose`].
    pub positions: Vec<[f32; 3]>,
    base: Vec<[f32; 3]>,
    object_ids: Vec<u32>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Texture modulation colour (the prim's packet colour; `0x80` neutral).
    pub colors: Vec<[u8; 3]>,
    /// `[r, g, b, flag]` per vertex: flag `255` textured, `0` untextured
    /// (`crate::packet_color::hybrid`'s layout).
    pub flat_rgba: Vec<u8>,
    /// Every triangle.
    pub indices: Vec<u32>,
    /// The textured triangles only.
    pub textured_indices: Vec<u32>,
    /// The untextured triangles only.
    pub untextured_indices: Vec<u32>,
    bodies: Vec<Range<usize>>,
}

impl DanceCastScene {
    /// Build the buffers for `models`, in order. A model with no mesh keeps
    /// an empty range, so body `i` is always range `i`.
    pub fn build(assets: &DanceCastAssets, models: &[DanceBodyModel]) -> Self {
        let mut s = Self {
            models: models.to_vec(),
            positions: Vec::new(),
            base: Vec::new(),
            object_ids: Vec::new(),
            uvs: Vec::new(),
            cba_tsb: Vec::new(),
            colors: Vec::new(),
            flat_rgba: Vec::new(),
            indices: Vec::new(),
            textured_indices: Vec::new(),
            untextured_indices: Vec::new(),
            bodies: Vec::new(),
        };
        for &m in models {
            let r = match assets.mesh(m) {
                Some(b) => s.push_tmd(&b.tmd, &b.raw),
                None => s.base.len()..s.base.len(),
            };
            s.bodies.push(r);
        }
        s.positions = s.base.clone();
        s
    }

    /// The models the buffers hold, body order.
    pub fn models(&self) -> &[DanceBodyModel] {
        &self.models
    }

    /// Vertex range of body `i`.
    pub fn body_range(&self, i: usize) -> Option<Range<usize>> {
        self.bodies.get(i).cloned()
    }

    fn push_tmd(&mut self, tmd: &legaia_tmd::Tmd, raw: &[u8]) -> Range<usize> {
        let (mesh, oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(tmd, raw);
        let flat = crate::packet_color::hybrid(&mesh, &shading);
        let start = self.base.len();
        self.base.extend_from_slice(&mesh.positions);
        self.object_ids.extend_from_slice(&oids);
        self.uvs.extend_from_slice(&mesh.uvs);
        self.cba_tsb.extend_from_slice(&mesh.cba_tsb);
        self.colors.extend_from_slice(&mesh.colors);
        self.flat_rgba.extend_from_slice(&flat);
        for tri in mesh.indices.as_chunks::<3>().0 {
            let t = tri.map(|i| i + start as u32);
            self.indices.extend_from_slice(&t);
            if shading.textured.get(tri[0] as usize).copied().unwrap_or(1) != 0 {
                self.textured_indices.extend_from_slice(&t);
            } else {
                self.untextured_indices.extend_from_slice(&t);
            }
        }
        start..self.base.len()
    }

    /// Pose every body for this frame. `bodies` is in the order the buffers
    /// were built for; a body whose clip does not resolve stands in its rest
    /// pose at its position.
    pub fn pose(&mut self, assets: &DanceCastAssets, bodies: &[DanceBodyFrame]) {
        for (i, b) in bodies.iter().enumerate() {
            let Some(range) = self.bodies.get(i).cloned() else {
                continue;
            };
            let parts = assets
                .mesh(b.model)
                .map(|m| m.tmd.objects.len())
                .unwrap_or(0);
            let clip = resolve_clip(&assets.anm, b);
            let xf: Vec<Option<[f32; 9]>> = (0..parts)
                .map(|p| {
                    let t = sample(&assets.anm, clip?, p)?;
                    let (sx, cx) = angle(t.r_x).sin_cos();
                    let (sy, cy) = angle(t.r_y).sin_cos();
                    let (sz, cz) = angle(t.r_z).sin_cos();
                    Some([
                        cx,
                        sx,
                        cy,
                        sy,
                        cz,
                        sz,
                        t.t_x as f32,
                        t.t_y as f32,
                        t.t_z as f32,
                    ])
                })
                .collect();
            let k = f32::from(b.scale) / 4096.0;
            let (wsy, wcy) = angle(i32::from(b.yaw)).sin_cos();
            let origin = b.pos.map(f32::from);
            for v in range {
                let [mut x, mut y, mut z] = self.base[v];
                if let Some(Some(t)) = xf.get(self.object_ids[v] as usize) {
                    let [cx, sx, cy, sy, cz, sz, tx, ty, tz] = *t;
                    let ny = y * cx - z * sx;
                    let nz = y * sx + z * cx;
                    y = ny;
                    z = nz;
                    let nx = x * cy + z * sy;
                    let nz = -x * sy + z * cy;
                    x = nx;
                    z = nz;
                    let nx = x * cz - y * sz;
                    let ny = x * sz + y * cz;
                    x = nx + tx;
                    y = ny + ty;
                    z += tz;
                }
                let (x, y, z) = (x * k, y * k, z * k);
                self.positions[v] = [
                    x * wcy + z * wsy + origin[0],
                    y + origin[1],
                    -x * wsy + z * wcy + origin[2],
                ];
            }
        }
    }
}

/// The per-host cache every dance host drives once a frame: the assets
/// (handed over when the host builds the venue), the buffers for the floor's
/// current cast, and a generation that moves whenever the static buffers do.
#[derive(Debug, Default)]
pub struct DanceCastSurface {
    assets: Option<Arc<DanceCastAssets>>,
    scene: Option<DanceCastScene>,
    generation: u32,
}

impl DanceCastSurface {
    /// Hand the surface the assets of a freshly built venue (or drop them).
    /// Bumps the generation: the buffers rebuild on the next frame.
    pub fn set_assets(&mut self, assets: Option<Arc<DanceCastAssets>>) {
        self.assets = assets;
        self.scene = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Whether assets are loaded.
    pub fn has_assets(&self) -> bool {
        self.assets.is_some()
    }

    /// One frame. `None` - and the cached buffers dropped - with no run or
    /// no assets. The buffers rebuild when the run's cast (its model list)
    /// changes, which bumps [`Self::generation`]; the pose runs every call.
    pub fn frame(&mut self, game: Option<&DanceGame>) -> Option<&DanceCastScene> {
        let (Some(game), Some(assets)) = (game, self.assets.clone()) else {
            if self.scene.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            return None;
        };
        let bodies = game.body_frames();
        if bodies.is_empty() {
            if self.scene.take().is_some() {
                self.generation = self.generation.wrapping_add(1);
            }
            return None;
        }
        let models: Vec<DanceBodyModel> = bodies.iter().map(|b| b.model).collect();
        if self.scene.as_ref().map(|s| s.models()) != Some(models.as_slice()) {
            self.scene = Some(DanceCastScene::build(&assets, &models));
            self.generation = self.generation.wrapping_add(1);
        }
        let scene = self.scene.as_mut()?;
        scene.pose(&assets, &bodies);
        Some(scene)
    }

    /// Bumped whenever the static buffers a host holds are stale.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The live scene, as the last [`Self::frame`] posed it.
    pub fn scene(&self) -> Option<&DanceCastScene> {
        self.scene.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::dance_cast::DanceClip;

    #[test]
    fn body_clip_record_is_the_placement_id_minus_one() {
        let c = DanceBodyClip::of(&DanceClip {
            anim_id: 0x23B,
            translucent: true,
            rate: 8,
        });
        assert_eq!(c.id, 0x3B);
        assert_eq!(c.record(), Some(0x3A));
        assert_eq!(DanceBodyClip::default().record(), None);
    }
}
