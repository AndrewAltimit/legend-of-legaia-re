//! Export the selected sub-asset through the existing writers: PNG
//! (`legaia_tim::write_png`), glTF binary (`legaia_actor_formats::scene_gltf`),
//! WAV (`legaia_vab::write_wav`), OBJ (positions + faces off
//! `legaia_tmd::mesh::tmd_to_mesh`), and the raw bytes.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::analyze::{Analysis, TimRef, TmdRef, VabRef};

/// Decode TIM `t` with palette `clut` to `(width, height, rgba)`.
pub fn tim_rgba(a: &Analysis, t: &TimRef, clut: usize) -> Result<(usize, usize, Vec<u8>)> {
    let tim = legaia_tim::parse(a.tim_bytes(t)).context("TIM parse")?;
    let clut = clut.min(tim.palette_count().saturating_sub(1));
    let rgba = legaia_tim::decode_rgba8(&tim, clut).context("TIM decode")?;
    Ok((tim.pixel_width(), tim.pixel_height(), rgba))
}

pub fn tim_png(a: &Analysis, t: &TimRef, clut: usize, out: &Path) -> Result<()> {
    let (w, h, rgba) = tim_rgba(a, t, clut)?;
    if w == 0 || h == 0 {
        bail!("empty TIM ({w}x{h})");
    }
    legaia_tim::write_png(out, w, h, &rgba)
}

/// Wavefront OBJ of the mesh, in raw PSX units (Y down), faces triangulated
/// the way the renderer splits quads.
pub fn tmd_obj(a: &Analysis, t: &TmdRef, out: &Path) -> Result<()> {
    let bytes = a.tmd_bytes(t);
    let tmd = legaia_tmd::parse(bytes).context("TMD parse")?;
    let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, bytes);
    let mut s = String::new();
    s.push_str(&format!(
        "# legaia-studio: PROT entry {} TMD @ {} 0x{:X}\n# raw PSX units, +Y down\n",
        a.index,
        t.origin.describe(),
        t.offset
    ));
    for p in &mesh.positions {
        s.push_str(&format!("v {} {} {}\n", p[0], p[1], p[2]));
    }
    for f in mesh.indices.as_chunks::<3>().0 {
        s.push_str(&format!("f {} {} {}\n", f[0] + 1, f[1] + 1, f[2] + 1));
    }
    std::fs::write(out, s).with_context(|| format!("write {}", out.display()))
}

/// Textured `.glb`: the mesh plus an atlas baked from the entry's own TIMs.
/// 1 PSX unit exports as 1 cm.
pub fn tmd_glb(a: &Analysis, t: &TmdRef, out: &Path) -> Result<()> {
    use legaia_actor_formats::scene_gltf::{SceneInstance, SceneMesh, build_scene_glb};
    let bytes = a.tmd_bytes(t);
    let tmd = legaia_tmd::parse(bytes).context("TMD parse")?;
    let (mesh, _obj, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, bytes);
    let n = mesh.positions.len();
    let mut flat_rgba = Vec::with_capacity(n * 4);
    for i in 0..n {
        let textured = shading.textured.get(i).copied().unwrap_or(1) != 0;
        let c = if textured {
            mesh.colors.get(i).copied().unwrap_or([0x80; 3])
        } else {
            shading.colors.get(i).copied().unwrap_or([0xC0; 3])
        };
        flat_rgba.extend_from_slice(&[c[0], c[1], c[2], if textured { 255 } else { 0 }]);
    }
    let scene_mesh = SceneMesh {
        name: format!("prot{:04}_tmd{:X}", a.index, t.offset),
        positions: mesh.positions.iter().flatten().copied().collect(),
        uvs: mesh.uvs.iter().flatten().copied().collect(),
        cba_tsb: mesh.cba_tsb.iter().flatten().copied().collect(),
        indices: mesh.indices,
        flat_rgba,
        morph_targets: Vec::new(),
    };
    let inst = SceneInstance {
        mesh: 0,
        translation: [0.0; 3],
        rot_y: 0.0,
        rotation: None,
        scale: 0.01,
    };
    let vram = a.vram();
    let glb = build_scene_glb(&scene_mesh.name.clone(), &[scene_mesh], &[inst], &vram)
        .context("mesh has no triangles to export")?;
    std::fs::write(out, glb).with_context(|| format!("write {}", out.display()))
}

/// Decode VAG sample `sample` of bank `v` to mono PCM.
pub fn vag_pcm(a: &Analysis, v: &VabRef, sample: usize) -> Result<Vec<i16>> {
    let span = v
        .report
        .vag_samples
        .get(sample)
        .context("sample out of range")?;
    let body = a
        .bytes
        .get(span.byte_offset..span.byte_offset + span.size)
        .context("sample body runs past the entry")?;
    if body.is_empty() {
        bail!("empty sample");
    }
    legaia_vab::decode_vag(body)
}

pub fn vag_wav(a: &Analysis, v: &VabRef, sample: usize, out: &Path) -> Result<()> {
    let pcm = vag_pcm(a, v, sample)?;
    let f = std::fs::File::create(out).with_context(|| format!("create {}", out.display()))?;
    legaia_vab::write_wav(
        std::io::BufWriter::new(f),
        &pcm,
        legaia_engine_audio::vab_bind::VAB_SAMPLE_RATE,
    )?;
    Ok(())
}

/// Every sample of bank `v` as `<stem>_NNN.wav` into `dir`. Returns the count.
pub fn vab_all_wavs(a: &Analysis, v: &VabRef, dir: &Path, stem: &str) -> Result<usize> {
    std::fs::create_dir_all(dir)?;
    let mut n = 0;
    for i in 0..v.report.vag_samples.len() {
        if vag_wav(a, v, i, &dir.join(format!("{stem}_{i:03}.wav"))).is_ok() {
            n += 1;
        }
    }
    Ok(n)
}

pub fn bytes(data: &[u8], out: &Path) -> Result<()> {
    std::fs::write(out, data).with_context(|| format!("write {}", out.display()))
}
