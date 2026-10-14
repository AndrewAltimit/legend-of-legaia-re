//! Native **world-export composition**: bake an [`AssembledScene`] into the
//! textured `.glb` + JSON manifest set the `legaia-engine export-glb`
//! subcommand writes for Unity / VRChat world building (see
//! `docs/tooling/vrchat-world-export.md`).
//!
//! Three artifact families per scene, matching retail's own layering:
//!
//! - the **world glb**: ground heightfield + terrain tiles + placed objects,
//!   every draw instanced at its resolved world transform with its coplanar
//!   lift, bound placements posed at **frame 0** of their object-bind clip
//!   (the native play-window's cheap static path);
//! - **NPC glbs**: one animated `.glb` per catalogued MAN placement, the
//!   scene TMD posed by its ANM record with every bone-count-matching clip in
//!   the scene bundle baked as a named glTF animation - spawn transforms go
//!   in the manifest, not the file, so a world builder places them;
//! - **animated-prop glbs**: each distinct `(env mesh, clip)` pair whose
//!   bind clip has more than one frame (windmill sails, doors), exported
//!   object-local with the clip, plus every placement transform in the
//!   manifest. The world glb keeps their frame-0 static twins, so these are
//!   opt-in upgrades, not required parts.
//!
//! Everything composes through the same kernels the browser pages render
//! with ([`legaia_engine_scene::scene_assembly`], [`legaia_engine_scene::npc_catalog`],
//! `legaia_actor_formats::scene_gltf` / `character_gltf`), so the exported files
//! match the on-screen viewers. The output contains Sony-derived asset data:
//! it is user-generated, local, and must never be committed or redistributed.

use legaia_actor_formats::character_gltf::{CharacterClip, build_character_glb_hybrid};
use legaia_actor_formats::player_anm::PlayerAnmBundle;
use legaia_actor_formats::scene_gltf::{SceneInstance, SceneMesh};
use legaia_engine_scene::npc_catalog::{NpcCatalog, scene_anm_bundle};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene::scene_assembly::{
    AssembledScene, build_hybrid_env_mesh, build_hybrid_env_mesh_posed, draw_rot_y_radians,
    draw_rotation_quat, draw_translation, is_sky_mesh,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};

mod animated_props;
mod items;
mod manifest;
mod npcs;
mod party;
mod traversal;
mod trigger_reach;
mod world;

pub use animated_props::*;
pub use items::*;
pub use manifest::*;
pub use npcs::*;
pub use party::*;
pub use traversal::*;
pub use trigger_reach::*;
pub use world::*;

/// Placement transforms of one animated prop: `(translation, rot_y radians,
/// full rotation quaternion)` per placed instance, all in the export frame.
/// The quaternion is [`draw_rotation_quat`] - the same one the world glb's
/// frame-0 twin node carries; `rot_y` is its yaw-only legacy field.
type PropInstances = Vec<([f32; 3], f32, [f32; 4])>;

/// Field/duel clip playback rate baked into exported animation timelines -
/// the observed retail field animator cadence (see the characters page's
/// export, `web-viewer::character`).
const CLIP_FPS: f32 = 14.0;

/// Export tuning.
#[derive(Debug, Clone, Copy)]
pub struct GlbExportOptions {
    /// Uniform scale applied to every instance transform (translations and
    /// mesh scale alike), so `1.0` exports raw PSX world units and e.g.
    /// `1.0 / 64.0` makes one 128-unit walk tile two glTF meters - about
    /// right for VRChat player scale.
    pub scale: f32,
    /// Keep the sky-backdrop shells ([`is_sky_mesh`]) in the world glb. The
    /// site viewers hide them (they read as a giant intersecting plane from
    /// outside); inside a VR world they can serve as the horizon dressing.
    pub include_sky: bool,
}

impl Default for GlbExportOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            include_sky: false,
        }
    }
}

/// Frame-`frame` bone offsets of scene ANM record `anim_id - 1`, under
/// retail's count-equality contract (`FUN_8001b964` refuses to draw when the
/// mesh chain and the clip disagree). `None` = pose nothing.
fn frame_bone_offsets(
    bundle: &PlayerAnmBundle,
    anim_id: u8,
    nobj: usize,
    frame: usize,
) -> Option<Vec<([i16; 3], [i16; 3])>> {
    let rec_idx = (anim_id as usize).checked_sub(1)?;
    let rec = bundle.record(rec_idx).ok()?;
    let bones = rec.bone_count as usize;
    if bones != nobj {
        return None;
    }
    let f = frame.min((rec.frame_count as usize).saturating_sub(1));
    Some(
        (0..bones)
            .map(|b| match bundle.bone_transform(rec_idx, f, b) {
                Some(t) => (
                    [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                    [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                ),
                None => ([0; 3], [0; 3]),
            })
            .collect(),
    )
}

/// Slug a dialog label into a filename fragment.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().take(24) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if (c == ' ' || c == '-' || c == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Bake the export scale onto a character/prop glb's scene-root nodes so
/// the file opens **world-sized** in any glTF consumer - dragging a prop
/// into a Unity scene next to the built world then just fits, instead of
/// coming in 64x giant in raw PSX units. Root-level only: the per-object
/// nodes and their animated translations sit under the root, so the whole
/// subtree scales as one. Returns the input unchanged if the container
/// does not parse (never expected for our own baker's output).
fn scale_glb_scene_roots(glb: Vec<u8>, scale: f32) -> Vec<u8> {
    let patched = (|| -> Option<Vec<u8>> {
        if glb.len() < 20 || &glb[0..4] != b"glTF" {
            return None;
        }
        let jlen = u32::from_le_bytes(glb[12..16].try_into().ok()?) as usize;
        let mut json: Value = serde_json::from_slice(glb.get(20..20 + jlen)?).ok()?;
        let rest = glb.get(20 + jlen..)?.to_vec(); // BIN chunk, verbatim
        let roots: Vec<usize> = json["scenes"][0]["nodes"]
            .as_array()?
            .iter()
            .filter_map(|v| v.as_u64().map(|u| u as usize))
            .collect();
        for r in roots {
            let node = json["nodes"].get_mut(r)?;
            let mut sc = [f64::from(scale); 3];
            if let Some(e) = node.get("scale").and_then(|v| v.as_array()).cloned() {
                for (k, s) in sc.iter_mut().enumerate() {
                    *s *= e.get(k).and_then(|v| v.as_f64()).unwrap_or(1.0);
                }
            }
            node["scale"] = json!(sc);
        }
        let mut jbytes = serde_json::to_vec(&json).ok()?;
        while jbytes.len() % 4 != 0 {
            jbytes.push(b' ');
        }
        let total = 12 + 8 + jbytes.len() + rest.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(jbytes.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&jbytes);
        out.extend_from_slice(&rest);
        Some(out)
    })();
    patched.unwrap_or(glb)
}
