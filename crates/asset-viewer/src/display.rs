//! The `Display` value (one screen of content) + the TMD-viewer camera
//! animation state + the PROT-entry classifier that produces a `Display`.

use crate::loaders::{decode_vab_sample, load_tim};
use crate::stage_view::LinesPayload;
use crate::tmd_view::VramMeshPayload;
use legaia_engine_audio::DEFAULT_INPUT_RATE;
use legaia_engine_render::glam::{Mat4, Vec3};
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

/// Camera + animation state for the TMD viewer. Keeps the model spinning
/// at a constant angular velocity around its centroid.
pub(crate) struct MeshView {
    /// World-space center of the mesh AABB; the camera looks at this point.
    center: Vec3,
    /// Distance from the camera to `center`. Picked so the mesh fits in
    /// the viewport with a comfortable margin.
    distance: f32,
    /// Wall-clock origin for the rotation animation.
    started_at: Instant,
}

impl MeshView {
    pub(crate) fn from_aabb(lo: [f32; 3], hi: [f32; 3]) -> Self {
        let center = Vec3::new(
            0.5 * (lo[0] + hi[0]),
            0.5 * (lo[1] + hi[1]),
            0.5 * (lo[2] + hi[2]),
        );
        let extent = Vec3::new(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let radius = (0.5 * extent.length()).max(1.0);
        // Frame the bounding sphere with ~30° horizontal half-angle.
        let distance = radius / (30f32.to_radians().tan()) * 1.4;
        Self {
            center,
            distance,
            started_at: Instant::now(),
        }
    }

    pub(crate) fn mvp(&self, aspect: f32) -> Mat4 {
        let angle = self.started_at.elapsed().as_secs_f32() * 0.5; // ~28 deg/s
        // PSX has Y-down geometry, so flip Y in the model matrix to make the
        // mesh appear right-side-up under a Y-up camera.
        let model = Mat4::from_rotation_y(angle) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        let eye = self.center + Vec3::new(0.0, 0.0, self.distance);
        let view = Mat4::look_at_rh(eye, self.center, Vec3::Y);
        let near = (self.distance * 0.05).max(0.1);
        let far = self.distance * 4.0 + 100.0;
        let proj = Mat4::perspective_rh(60f32.to_radians(), aspect.max(0.01), near, far);
        proj * view * model
    }
}

/// Try to produce a [`Display`] for one PROT entry by walking the known
/// sub-formats in priority order. Returns `None` if nothing renderable
/// could be extracted (caller advances past it).
pub(crate) fn display_for_prot_entry(name: &str, bytes: &[u8]) -> Option<Display> {
    let report = legaia_asset::categorize::classify(bytes);
    let title = format!("{}  ({}, {} bytes)", name, report.class.name(), bytes.len());

    // 1. TIM passthrough (first u32 == 0x00000010).
    if report.class == legaia_asset::categorize::Class::TimPassthrough
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
    if report.class == legaia_asset::categorize::Class::TimPack {
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
    if report.class == legaia_asset::categorize::Class::DataFieldStreaming
        && let Ok(stream) = legaia_asset::parse_streaming(bytes, 4096)
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
            let Ok(items) = legaia_asset::pack::extract_pack(pack_data) else {
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
    // dominant scene-asset shape (148 PROT entries). Render flat-shaded -
    // no sibling TIM dir means no texturing, but the geometry is the
    // distinctive visual signal.
    if report.class == legaia_asset::categorize::Class::SceneTmdStream
        && let Some(s) = legaia_asset::scene_tmd_stream::detect(bytes)
        && let Ok(tmd) = legaia_tmd::parse(&bytes[s.tmd_range()])
    {
        let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &bytes[s.tmd_range()]);
        if !mesh.indices.is_empty() {
            return Some(Display {
                title: format!(
                    "{} [scene_tmd_stream: {} obj, {} verts, {} tris{}]",
                    title,
                    tmd.objects.len(),
                    mesh.positions.len(),
                    mesh.indices.len() / 3,
                    if s.tail_chunks.is_empty() {
                        String::new()
                    } else {
                        format!(", +{} tail chunks", s.tail_chunks.len())
                    },
                ),
                image: None,
                audio: None,
                mesh: Some((mesh.positions, mesh.indices)),
                vram_mesh: None,
                lines: None,
            });
        }
    }

    // 5. Scene-VAB-prefixed stream: leading VAB at offset 4 (217 PROT
    // entries - the dominant distributed-VAB carrier). Play sample 0 of
    // the embedded bank.
    if report.class == legaia_asset::categorize::Class::SceneVabStream
        && let Some(s) = legaia_asset::scene_vab_stream::detect(bytes)
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
