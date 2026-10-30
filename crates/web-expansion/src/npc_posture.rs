//! Measure the pose of every clip a scene's NPC placements can play: the
//! source of `legaia_expansion_format::npc_postures`' committed table, and
//! the workbench NPC form's clip hint.
//!
//! Each clip whose bone count matches the placement model's object count is
//! posed frame by frame through the model ([`tmd_to_vram_mesh_posed_rot`],
//! the per-object `Rz·Ry·Rx` + offset both hosts draw with). Three numbers
//! come out of the posed vertices, PSX `y` down:
//!
//! - `height`: the mean vertical extent over the clip's frames.
//! - `raise`: how far the clip's lowest point (its largest `y` over every
//!   frame) sits above the model's standing floor.
//! - `legs`: the 90th-percentile travel, from frame 0, of the vertices in
//!   the lower 30 % of frame 0's extent.
//!
//! The model's references are the upper-quartile height and floor over all
//! of its clips (most clips stand): a bundle mixes clips authored for other
//! rigs of the same bone count, whose origins sit at other heights, so the
//! floor is the model's own rather than the actor origin. The classes follow
//! [`classify`]'s thresholds.
//!
//! [`tmd_to_vram_mesh_posed_rot`]: legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use legaia_engine_scene::npc_catalog::{scene_anm_bundle, scene_npc_picks};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_expansion_format::npc_postures::{PlacementPostures, Posture};
use serde::Serialize;

/// A seated clip's lowest point sits at least this far (world units) above
/// the model's standing floor.
pub const SEATED_RAISE: f32 = 20.0;
/// ... and its height is at most this share of the standing height.
pub const SEATED_HEIGHT: f32 = 0.85;
/// A crouch is at most this share of the standing height, unlifted.
pub const CROUCHED_HEIGHT: f32 = 0.8;
/// A walk moves the lower body at least this far (world units).
pub const WALKING_LEGS: f32 = 25.0;

/// One clip's measurement.
#[derive(Debug, Clone, Serialize)]
pub struct ClipMetrics {
    /// Clip id (ANM record + 1).
    pub clip: u8,
    pub frames: u16,
    pub height: f32,
    pub raise: f32,
    pub legs: f32,
    pub posture: Posture,
}

/// [`ClipMetrics`]' class from the numbers and the model's standing
/// height.
pub fn classify(height: f32, raise: f32, legs: f32, standing_height: f32) -> Posture {
    let share = if standing_height > 0.0 {
        height / standing_height
    } else {
        1.0
    };
    if raise >= SEATED_RAISE && share <= SEATED_HEIGHT {
        Posture::Seated
    } else if share <= CROUCHED_HEIGHT {
        Posture::Crouched
    } else if legs >= WALKING_LEGS {
        Posture::Walking
    } else {
        Posture::Standing
    }
}

/// One placement's measured clips.
#[derive(Debug, Clone, Serialize)]
pub struct PlacementMetrics {
    pub index: u16,
    pub model_index: u8,
    /// The placement's own clip id, `0` for none.
    pub clip: u8,
    pub clips: Vec<ClipMetrics>,
}

impl PlacementMetrics {
    /// The table row.
    pub fn postures(&self, scene: &str) -> PlacementPostures {
        let mut p = PlacementPostures {
            scene: scene.to_string(),
            index: self.index,
            clip: self.clip,
            ..Default::default()
        };
        for c in &self.clips {
            match c.posture {
                Posture::Standing => p.standing.push(c.clip),
                Posture::Walking => p.walking.push(c.clip),
                Posture::Seated => p.seated.push(c.clip),
                Posture::Crouched => p.crouched.push(c.clip),
            }
        }
        p
    }
}

struct Raw {
    clip: u8,
    frames: u16,
    height: f32,
    floor: f32,
    legs: f32,
}

fn upper_quartile(mut v: Vec<f32>) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() * 3 / 4).copied().unwrap_or(0.0)
}

/// Measure every posable placement of scene `name`: a scene placement (not
/// a party member or save point) whose model has more than one object, in
/// a scene that ships an ANM bundle. Placements are listed in MAN order.
pub fn measure_scene(index: &ProtIndex, name: &str) -> Result<Vec<PlacementMetrics>> {
    let assembled =
        legaia_engine_scene::scene_assembly::assemble_field_scene_with(index, name, &|_, _| {
            Default::default()
        })
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let scene = Scene::load(index, name)?;
    let Some(bundle) = scene_anm_bundle(&scene) else {
        return Ok(Vec::new());
    };
    let picks = scene_npc_picks(index, name).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut by_model: BTreeMap<u8, Vec<ClipMetrics>> = BTreeMap::new();
    let mut out = Vec::new();
    for p in picks {
        if p.special {
            continue;
        }
        let Some(t) = assembled.res.tmds.get(usize::from(p.model_index)) else {
            continue;
        };
        let nobj = t.tmd.objects.len();
        if nobj < 2 {
            continue;
        }
        let clips = match by_model.get(&p.model_index) {
            Some(c) => c.clone(),
            None => {
                let c = measure_model(&bundle, &t.tmd, &t.raw, nobj)
                    .with_context(|| format!("{name} model {}", p.model_index))?;
                by_model.insert(p.model_index, c.clone());
                c
            }
        };
        if clips.is_empty() {
            continue;
        }
        out.push(PlacementMetrics {
            index: p.index,
            model_index: p.model_index,
            clip: p.anim_id,
            clips,
        });
    }
    Ok(out)
}

fn measure_model(
    bundle: &legaia_actor_formats::player_anm::PlayerAnmBundle,
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    nobj: usize,
) -> Result<Vec<ClipMetrics>> {
    let mut rows = Vec::new();
    for r in 0..bundle.record_count as usize {
        let Ok(rec) = bundle.record(r) else { continue };
        if usize::from(rec.bone_count) != nobj || rec.frame_count == 0 || r >= 255 {
            continue;
        }
        let frames = usize::from(rec.frame_count);
        let mut base: Vec<[f32; 3]> = Vec::new();
        let mut legs_mask: Vec<bool> = Vec::new();
        let mut travel: Vec<f32> = Vec::new();
        let (mut height_sum, mut floor, mut counted) = (0f32, f32::MIN, 0usize);
        for f in 0..frames {
            let offs: Vec<([i16; 3], [i16; 3])> = (0..nobj)
                .map(|b| match bundle.bone_transform(r, f, b) {
                    Some(t) => (
                        [t.t_x as i16, t.t_y as i16, t.t_z as i16],
                        [t.r_x as i16, t.r_y as i16, t.r_z as i16],
                    ),
                    None => ([0; 3], [0; 3]),
                })
                .collect();
            let m = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(tmd, raw, &offs);
            if m.positions.is_empty() {
                continue;
            }
            let (lo, hi) = m
                .positions
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    (lo.min(p[1]), hi.max(p[1]))
                });
            height_sum += hi - lo;
            floor = floor.max(hi);
            counted += 1;
            if base.is_empty() {
                legs_mask = m
                    .positions
                    .iter()
                    .map(|p| p[1] > hi - 0.3 * (hi - lo))
                    .collect();
                travel = vec![0.0; m.positions.len()];
                base = m.positions;
            } else if m.positions.len() == base.len() {
                for (i, p) in m.positions.iter().enumerate() {
                    let d = (0..3)
                        .map(|k| (p[k] - base[i][k]).powi(2))
                        .sum::<f32>()
                        .sqrt();
                    travel[i] = travel[i].max(d);
                }
            }
        }
        if counted == 0 {
            continue;
        }
        let mut legs: Vec<f32> = travel
            .iter()
            .zip(&legs_mask)
            .filter(|(_, m)| **m)
            .map(|(d, _)| *d)
            .collect();
        legs.sort_by(|a, b| a.total_cmp(b));
        rows.push(Raw {
            clip: (r + 1) as u8,
            frames: rec.frame_count,
            height: height_sum / counted as f32,
            floor,
            legs: legs.get(legs.len() * 9 / 10).copied().unwrap_or(0.0),
        });
    }
    let standing_height = upper_quartile(rows.iter().map(|r| r.height).collect());
    let standing_floor = upper_quartile(rows.iter().map(|r| r.floor).collect());
    Ok(rows
        .into_iter()
        .map(|r| {
            let raise = standing_floor - r.floor;
            ClipMetrics {
                clip: r.clip,
                frames: r.frames,
                height: r.height,
                raise,
                legs: r.legs,
                posture: classify(r.height, raise, r.legs, standing_height),
            }
        })
        .collect())
}

/// The committed table's text for `table`: a header naming the measurement
/// and thresholds, then one `[[npc]]` block per placement.
pub fn table_toml(table: &legaia_expansion_format::npc_postures::PostureTable) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "# The measured pose of each clip a borrowed NPC placement can play.\n\
         # Generated by `cargo run -p legaia-web-expansion --example npc_postures --\n\
         # extracted <scene>...` (legaia_web_expansion::npc_posture); a disc-gated\n\
         # test checks it against the disc. Disc coordinates and clip ids only.\n\
         #\n\
         # seated: lowest point >= {SEATED_RAISE} units above the model's standing floor and\n\
         # height <= {SEATED_HEIGHT} of its standing height; crouched: height <= {CROUCHED_HEIGHT}\n\
         # unlifted; walking: lower body travels >= {WALKING_LEGS} units; else standing."
    );
    for p in &table.npc {
        let list = |v: &[u8]| v.iter().map(u8::to_string).collect::<Vec<_>>().join(", ");
        let _ = writeln!(
            s,
            "\n[[npc]]\nscene = \"{}\"\nindex = {}\nclip = {}",
            p.scene, p.index, p.clip
        );
        for (k, v) in [
            ("standing", &p.standing),
            ("walking", &p.walking),
            ("seated", &p.seated),
            ("crouched", &p.crouched),
        ] {
            if !v.is_empty() {
                let _ = writeln!(s, "{k} = [{}]", list(v));
            }
        }
    }
    s
}
