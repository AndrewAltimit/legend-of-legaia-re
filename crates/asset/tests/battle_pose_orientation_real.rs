//! Disc-gated **orientation** oracle for the assembled battle party mesh.
//!
//! The battle form is assembled per character from the player battle files
//! (`docs/formats/character-mesh.md`), and its rest pose is frame 0 of the
//! character's own idle stream inside the same file - not PROT 1203. That
//! makes "does the assembled + posed mesh actually stand up, facing the
//! enemy seats" a pure data question, answerable without a GPU: pose the
//! mesh and measure its AABB.
//!
//! The invariant matters because the on-screen failure it guards is not
//! subtle - the humanoid battle clips split cleanly into upright ones (idle,
//! walk, the weapon swings) and prone ones (knockdown / get-up), and a bug
//! that plays the prone family when the upright one was staged renders the
//! party member lying on the ground for the whole turn. The second test
//! pins that the prone family really is prone, so the first test's threshold
//! is not vacuous.
//!
//! Set `LEGAIA_POSE_DUMP_DIR` to also write orthographic front/side PNG
//! renders of each posed mesh (a diagnostic aid, off by default).
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset.

use std::path::{Path, PathBuf};

use legaia_asset::monster_archive::MonsterAnimation;
use legaia_asset::{battle_char_assembly, battle_data_pack};

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

/// The three humanoid party characters (Vahn / Noa / Gala) and their
/// extraction PROT entry prefixes. `0866` (the fourth player battle file) is
/// deliberately excluded: it is a quadruped, so the upright invariant does
/// not describe it.
const HUMANOIDS: [(usize, &str); 3] = [(0, "0863_"), (1, "0864_"), (2, "0865_")];

fn player_file(prot_dir: &Path, prefix: &str) -> Option<Vec<u8>> {
    let entry = std::fs::read_dir(prot_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .find(|e| e.file_name().to_string_lossy().starts_with(prefix))?;
    Some(std::fs::read(entry.path()).expect("read player file"))
}

/// Assembled mesh (default / unequipped sections) + its decoded action
/// streams, expanded per assembled object.
struct Assembled {
    tmd: legaia_tmd::Tmd,
    raw: Vec<u8>,
    idle: MonsterAnimation,
    clips: Vec<MonsterAnimation>,
}

fn assemble(prot_dir: &Path, prefix: &str) -> Option<Assembled> {
    let raw = player_file(prot_dir, prefix)?;
    let pack = battle_data_pack::parse(&raw).expect("parse pack");
    let asm = battle_char_assembly::assemble_character(&raw, &pack, &[0u8; 5])
        .expect("assemble character");
    let tmd = legaia_tmd::parse(&asm.tmd).expect("parse assembled tmd");
    let idle = battle_char_assembly::idle_battle_animation(&raw)
        .expect("idle decode")
        .expect("idle present");
    let idle = battle_char_assembly::expand_animation_for_objects(&idle, &asm.anm_bones);
    let clips = battle_char_assembly::battle_animations(&raw)
        .expect("action streams")
        .iter()
        .map(|a| battle_char_assembly::expand_animation_for_objects(a, &asm.anm_bones))
        .collect();
    Some(Assembled {
        tmd,
        raw: asm.tmd,
        idle,
        clips,
    })
}

/// Pose `asm` at keyframe `frame` of `anim` exactly as the renderer does
/// (`tmd_to_vram_mesh_posed_rot` over the expanded per-object stream) and
/// return the resulting AABB extents `(x, y, z)`.
fn posed_extents(asm: &Assembled, anim: &MonsterAnimation, frame: usize) -> ([f32; 3], [f32; 3]) {
    let f = &anim.frames[frame.min(anim.frames.len() - 1)];
    let bones: Vec<([i16; 3], [i16; 3])> = f
        .iter()
        .map(|p| ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16]))
        .collect();
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot(&asm.tmd, &asm.raw, &bones);
    let (lo, hi) = mesh.aabb();
    if let Some(dir) = std::env::var_os("LEGAIA_POSE_DUMP_DIR").map(PathBuf::from) {
        std::fs::create_dir_all(&dir).ok();
        let tag = format!("a{:02x}_f{frame}", anim.action_id);
        dump_ortho(
            &dir.join(format!("{tag}_front.png")),
            &mesh.positions,
            &mesh.indices,
            (0, 1, 2),
        );
        dump_ortho(
            &dir.join(format!("{tag}_side.png")),
            &mesh.positions,
            &mesh.indices,
            (2, 1, 0),
        );
    }
    (lo, hi)
}

/// Orthographic software raster of a posed mesh along one axis pair, as a
/// greyscale PNG (nearer = brighter). Diagnostic aid only - the assertions
/// read the AABB, not these pixels.
fn dump_ortho(path: &Path, positions: &[[f32; 3]], indices: &[u32], axes: (usize, usize, usize)) {
    const W: usize = 256;
    const H: usize = 256;
    let (ax, ay, az) = axes;
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in positions {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let span = (hi[ax] - lo[ax]).max(hi[ay] - lo[ay]).max(1.0);
    // PSX Y points down, so a raw row index already puts the head at the top.
    let to_px = |p: &[f32; 3]| -> (f32, f32, f32) {
        (
            (p[ax] - lo[ax]) / span * (W as f32 - 1.0),
            (p[ay] - lo[ay]) / span * (H as f32 - 1.0),
            p[az],
        )
    };
    let mut depth = vec![f32::MAX; W * H];
    let mut img = vec![0u8; W * H];
    for tri in indices.chunks_exact(3) {
        let v: Vec<(f32, f32, f32)> = tri.iter().map(|&i| to_px(&positions[i as usize])).collect();
        let (x0, y0) = (v[0].0, v[0].1);
        let (x1, y1) = (v[1].0, v[1].1);
        let (x2, y2) = (v[2].0, v[2].1);
        let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if area.abs() < 1e-6 {
            continue;
        }
        let minx = x0.min(x1).min(x2).floor().max(0.0) as usize;
        let maxx = (x0.max(x1).max(x2).ceil() as usize).min(W - 1);
        let miny = y0.min(y1).min(y2).floor().max(0.0) as usize;
        let maxy = (y0.max(y1).max(y2).ceil() as usize).min(H - 1);
        for py in miny..=maxy {
            for px in minx..=maxx {
                let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                let w0 = ((x1 - fx) * (y2 - fy) - (x2 - fx) * (y1 - fy)) / area;
                let w1 = ((x2 - fx) * (y0 - fy) - (x0 - fx) * (y2 - fy)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * v[0].2 + w1 * v[1].2 + w2 * v[2].2;
                let idx = py * W + px;
                if z < depth[idx] {
                    depth[idx] = z;
                    let t = ((z - lo[az]) / (hi[az] - lo[az]).max(1.0)).clamp(0.0, 1.0);
                    img[idx] = (40.0 + (1.0 - t) * 215.0) as u8;
                }
            }
        }
    }
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
    enc.set_color(png::ColorType::Grayscale);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(&img)
        .expect("png data");
}

#[test]
fn assembled_rest_pose_stands_upright() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    for (cslot, prefix) in HUMANOIDS {
        let Some(asm) = assemble(&prot_dir, prefix) else {
            eprintln!("[skip] char {cslot} player file missing");
            continue;
        };
        let (lo, hi) = posed_extents(&asm, &asm.idle, 0);
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        eprintln!("char {cslot} idle f0: lo={lo:?} hi={hi:?} extent={ext:?}");
        // PSX world Y points DOWN, so an upright character occupies negative
        // Y (head) down to ~0 (feet on the stage plane).
        assert!(
            lo[1] < -200.0,
            "char {cslot}: head should sit well above the stage plane, lo.y = {}",
            lo[1]
        );
        assert!(
            hi[1].abs() < 60.0,
            "char {cslot}: feet should rest near y = 0, hi.y = {}",
            hi[1]
        );
        // Standing: the vertical extent dominates both horizontal ones.
        assert!(
            ext[1] > ext[0] * 1.5 && ext[1] > ext[2],
            "char {cslot}: rest pose is not upright, extent = {ext:?}"
        );
    }
}

#[test]
fn knockdown_and_getup_clips_are_prone() {
    // The counterpart of the upright invariant: the reaction family really
    // does lay the character out flat, which is why staging the wrong one
    // over an attack turn is visible from across the arena. Action tags: 4 =
    // knockdown, 5 = get-up (`battle_char_assembly::action_slot_label`).
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    for (cslot, prefix) in HUMANOIDS {
        let Some(asm) = assemble(&prot_dir, prefix) else {
            continue;
        };
        let knockdown = asm
            .clips
            .iter()
            .find(|c| c.action_id == 4)
            .expect("knockdown clip");
        // Last keyframe = the downed pose the chain holds.
        let (lo, hi) = posed_extents(&asm, knockdown, knockdown.frames.len() - 1);
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        eprintln!("char {cslot} knockdown last: extent={ext:?}");
        assert!(
            ext[1] < ext[2],
            "char {cslot}: knockdown end pose should lie along Z, extent = {ext:?}"
        );
    }
}
