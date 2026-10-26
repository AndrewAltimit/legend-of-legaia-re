//! The `Display` value (one screen of content) + the TMD-viewer camera
//! animation state + the PROT-entry classifier that produces a `Display`.

use crate::loaders::{decode_vab_sample, load_tim};
use crate::stage_view::LinesPayload;
use crate::tmd_view::VramMeshPayload;
use glam::{Mat4, Vec3};
use legaia_engine_audio::DEFAULT_INPUT_RATE;
use std::time::Instant;

/// One screen of content the viewer can display.
pub(crate) struct Display {
    pub(crate) title: String,
    /// `(rgba, width, height)` if this entry has visual content.
    pub(crate) image: Option<(Vec<u8>, u32, u32)>,
    /// `(pcm, sample_rate)` if this entry has audible content.
    pub(crate) audio: Option<(Vec<i16>, u32)>,
    /// `(positions, indices)` if this entry is a 3D mesh (TMD viewer mode).
    /// Mutually exclusive with `vram_mesh`.
    pub(crate) mesh: Option<(Vec<[f32; 3]>, Vec<u32>)>,
    /// VRAM-mesh payload for proper PSX texture lookup (multi-page,
    /// per-prim CBA/TSB). Mutually exclusive with `mesh`.
    pub(crate) vram_mesh: Option<VramMeshPayload>,
    /// Wireframe payload (positions + per-vertex color + line indices) for
    /// the stage-geometry viewer. Mutually exclusive with `mesh`/`vram_mesh`.
    pub(crate) lines: Option<LinesPayload>,
}

impl Display {
    pub(crate) fn empty(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            image: None,
            audio: None,
            mesh: None,
            vram_mesh: None,
            lines: None,
        }
    }
}

/// Camera + animation state for the TMD viewer: the orbit pose of the
/// static site's mesh viewers and Legaia Studio (eye slightly above the
/// model, looking down, three-quarter front), turning slowly about the
/// model's vertical axis.
pub(crate) struct MeshView {
    /// Centre of the mesh AABB in PSX model space (`+Y` down); the camera
    /// looks at this point.
    center: Vec3,
    /// Distance from the camera to `center`.
    distance: f32,
    /// Wall-clock origin for the rotation animation.
    started_at: Instant,
}

/// Vertical field of view, radians - the site viewers' and Studio's.
const MESH_FOV_Y: f32 = 1.1;
/// Eye elevation above the model, radians (positive looks down).
const MESH_PITCH: f32 = 0.2;
/// Starting turn past the model's front (meshes face PSX `+Z`), radians.
const MESH_YAW: f32 = 0.5;
/// Eye distance in bounding-sphere radii.
const MESH_DISTANCE_RADII: f32 = 2.6;
/// Turn rate, radians per second.
const MESH_SPIN: f32 = 0.5;

impl MeshView {
    pub(crate) fn from_aabb(lo: [f32; 3], hi: [f32; 3]) -> Self {
        let center = Vec3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let extent = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let radius = (0.5 * extent.length()).max(1.0);
        Self {
            center,
            distance: radius * MESH_DISTANCE_RADII,
            started_at: Instant::now(),
        }
    }

    /// The eye in PSX model space `secs` into the turn: above the centre
    /// (PSX `-Y`) by [`MESH_PITCH`], on the model's front (`+Z`) side.
    fn eye_psx(&self, secs: f32) -> Vec3 {
        let yaw = MESH_YAW + secs * MESH_SPIN;
        let (sp, cp) = MESH_PITCH.sin_cos();
        self.center + self.distance * Vec3::new(-yaw.sin() * cp, -sp, yaw.cos() * cp)
    }

    pub(crate) fn mvp(&self, aspect: f32) -> Mat4 {
        self.mvp_at(self.started_at.elapsed().as_secs_f32(), aspect)
    }

    fn mvp_at(&self, secs: f32, aspect: f32) -> Mat4 {
        // PSX model space is +Y down with the GTE's screen = (x right, y
        // down, z into the screen). Turning it half a turn about X (flip
        // both Y and Z) maps it onto the renderer's Y-up right-handed space
        // without mirroring the model (a Y-only flip would).
        let flip = Vec3::new(1.0, -1.0, -1.0);
        let model = Mat4::from_scale(flip);
        let eye = self.eye_psx(secs) * flip;
        let center = self.center * flip;
        let view = Mat4::look_at_rh(eye, center, Vec3::Y);
        let near = (self.distance * 0.05).max(0.1);
        let far = self.distance * 4.0 + 100.0;
        let proj = Mat4::perspective_rh(MESH_FOV_Y, aspect.max(0.01), near, far);
        proj * view * model
    }
}

/// Try to produce a [`Display`] for one PROT entry by walking the known
/// sub-formats in priority order. Returns `None` if nothing renderable
/// could be extracted (caller advances past it).
///
/// `backdrop_second_copy` is the transform retail applies to a
/// `scene_tmd_stream` backdrop's second copy, resolved from the per-stage
/// table in `SCUS_942.54`. `None` means the executable wasn't available,
/// in which case the preview uses retail's default (a half turn) and the
/// status line says the transform is unresolved rather than asserting one.
pub(crate) fn display_for_prot_entry(
    name: &str,
    bytes: &[u8],
    backdrop_second_copy: Option<legaia_scene_formats::battle_backdrop::SecondCopy>,
) -> Option<Display> {
    let report = legaia_disc_audit::categorize::classify(bytes);
    let title = format!("{}  ({}, {} bytes)", name, report.class.name(), bytes.len());

    // 1. TIM passthrough (first u32 == 0x00000010).
    if report.class == legaia_disc_audit::categorize::Class::TimPassthrough
        && let Ok((rgba, w, h)) = load_tim(bytes, 0)
    {
        return Some(Display {
            title,
            image: Some((rgba, w, h)),
            audio: None,
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // 2. Standalone TIM-pack: take the first item.
    if report.class == legaia_disc_audit::categorize::Class::TimPack {
        let items = legaia_prot::timpack::unpack(bytes);
        for item in &items {
            if let Ok((rgba, w, h)) = load_tim(item, 0) {
                return Some(Display {
                    title: format!("{} [pack:0/{}]", title, items.len()),
                    image: Some((rgba, w, h)),
                    audio: None,
                    mesh: None,
                    vram_mesh: None,
                    lines: None,
                });
            }
        }
    }

    // 3. DATA_FIELD streaming: walk chunks, find a TIM_LIST chunk with a
    // first sub-TIM that decodes.
    if report.class == legaia_disc_audit::categorize::Class::DataFieldStreaming
        && let Ok(stream) = legaia_container_formats::parse_streaming(bytes, 4096)
    {
        for chunk in &stream.chunks {
            if chunk.type_byte != 0x01 {
                continue;
            }
            let data_start = chunk.header_offset + 4;
            let data_end = data_start + chunk.size as usize;
            if data_end > bytes.len() {
                continue;
            }
            let pack_data = &bytes[data_start..data_end];
            let Ok(items) = legaia_container_formats::pack::extract_pack(pack_data) else {
                continue;
            };
            for item in &items {
                if let Ok((rgba, w, h)) = load_tim(item, 0) {
                    return Some(Display {
                        title: format!("{} [stream:TIM_LIST]", title),
                        image: Some((rgba, w, h)),
                        audio: None,
                        mesh: None,
                        vram_mesh: None,
                        lines: None,
                    });
                }
            }
        }
    }

    // 4. Scene-TMD-prefixed stream: leading bare TMD at offset 4 is the
    // dominant scene-asset shape (182 PROT entries). Render flat-shaded -
    // no sibling TIM dir means no texturing, but the geometry is the
    // distinctive visual signal.
    //
    // Placed the way retail places it, which is *not* "every object, once":
    // the backdrop actor drops object 1, and a second actor draws the same
    // mesh under a per-stage transform that closes the shell. Drawing the
    // raw file instead shows a half bowl with a stray prop in it, which has
    // repeatedly been misread as a broken parse. See
    // `legaia_scene_formats::battle_backdrop`.
    if report.class == legaia_disc_audit::categorize::Class::SceneTmdStream
        && let Some(s) = legaia_scene_formats::scene_tmd_stream::detect(bytes)
        && let Ok(tmd) = legaia_tmd::parse(&bytes[s.tmd_range()])
    {
        use legaia_scene_formats::battle_backdrop as backdrop;
        let drawn = backdrop::drawn_objects_tmd(&tmd);
        let mut mesh = legaia_tmd::mesh::tmd_to_mesh(&drawn, &bytes[s.tmd_range()]);
        let second = backdrop_second_copy.unwrap_or(backdrop::SecondCopy::HalfTurn);
        let copy = mesh.clone();
        mesh.append_scaled(&copy, second.scale());
        // Retail's procedural floor, appended after the second copy so the
        // shell transform never reaches it. This preview is untextured, so
        // the tile-residency gate the textured host applies would say
        // nothing here - the grid is drawn for its geometry, which is what
        // the flat preview shows anyway.
        let grid = backdrop::build_ground_grid();
        let (shell_verts, shell_tris) = (mesh.positions.len(), mesh.indices.len() / 3);
        let base = mesh.positions.len() as u32;
        mesh.positions.extend_from_slice(&grid.positions);
        mesh.indices.extend(grid.indices.iter().map(|i| i + base));
        if !mesh.indices.is_empty() {
            let shape = legaia_scene_formats::scene_tmd_stream::shell_shape(&tmd);
            let placement = format!(
                " - {}",
                backdrop::describe_placement(shape.as_ref(), backdrop_second_copy)
            );
            return Some(Display {
                title: format!(
                    "{} [scene_tmd_stream: {} obj ({} drawn x2), {} verts, {} tris \
                     + {} ground-grid tris{}{}]",
                    title,
                    tmd.objects.len(),
                    drawn.objects.len(),
                    shell_verts,
                    shell_tris,
                    grid.indices.len() / 3,
                    if s.tail_chunks.is_empty() {
                        String::new()
                    } else {
                        format!(", +{} tail chunks", s.tail_chunks.len())
                    },
                    placement,
                ),
                image: None,
                audio: None,
                mesh: Some((mesh.positions, mesh.indices)),
                vram_mesh: None,
                lines: None,
            });
        }
    }

    // 5. Scene-VAB-prefixed stream: leading VAB at offset 4 (218 PROT
    // entries - the dominant distributed-VAB carrier). Play sample 0 of
    // the embedded bank.
    if report.class == legaia_disc_audit::categorize::Class::SceneVabStream
        && let Some(s) = legaia_container_formats::scene_vab_stream::detect(bytes)
        && let Ok(pcm) = decode_vab_sample(bytes, s.vab_range().start, 0)
    {
        return Some(Display {
            title: format!(
                "{} [scene_vab_stream: VAB v{}, ps={}, ts={}, sample 0]",
                title, s.vab_version, s.vab_ps, s.vab_ts
            ),
            image: None,
            audio: Some((pcm, DEFAULT_INPUT_RATE)),
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // 6. VAB bank fallback: scan the entry for a VAB header (the bank may
    // live at a non-zero offset inside a larger PROT entry - battle_data
    // and level_up entries hold theirs deep inside) and play sample 0 of
    // the first one we find.
    let vab_offsets = legaia_vab::find_vabs(bytes);
    if let Some(&off) = vab_offsets.first()
        && let Ok(pcm) = decode_vab_sample(bytes, off, 0)
    {
        return Some(Display {
            title: format!("{} [vab @ 0x{:X}, sample 0]", title, off),
            image: None,
            audio: Some((pcm, DEFAULT_INPUT_RATE)),
            mesh: None,
            vram_mesh: None,
            lines: None,
        });
    }

    // Nothing displayable.
    Some(Display::empty(title))
}

#[cfg(test)]
mod mesh_view_tests {
    use super::*;

    /// The preview opens looking down on the model: its top (PSX `-Y`)
    /// is nearer the eye than its feet and draws above them, and a point
    /// off the model's right hand (PSX `+X` seen from the front) draws on
    /// screen-left, as a front view of an unmirrored model does.
    #[test]
    fn opens_from_above_unmirrored() {
        let v = MeshView::from_aabb([-50.0, -100.0, -50.0], [50.0, 0.0, 50.0]);
        let m = v.mvp_at(0.0, 1.0);
        let clip = |p: [f32; 3]| m * glam::Vec4::new(p[0], p[1], p[2], 1.0);
        let head = clip([0.0, -100.0, 0.0]);
        let feet = clip([0.0, 0.0, 0.0]);
        assert!(head.w < feet.w, "head nearer the eye");
        assert!(head.y / head.w > feet.y / feet.w, "head above feet");
        // Head-on (yaw 0) a model facing +Z has its +X side on the viewer's
        // left; at the starting three-quarter turn it still is.
        let left = clip([40.0, -50.0, 0.0]);
        let right = clip([-40.0, -50.0, 0.0]);
        assert!(left.x / left.w < right.x / right.w);
    }
}
