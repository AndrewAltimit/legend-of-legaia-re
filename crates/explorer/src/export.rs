//! Export the selected sub-asset through the existing writers: PNG
//! (`psx_tim::write_png`), glTF binary (`legaia_actor_formats::scene_gltf`),
//! WAV (`psx_vab::write_wav`), OBJ (positions + faces off
//! `legaia_tmd::mesh::tmd_to_mesh`), and the raw bytes.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::analyze::{Analysis, TimRef, TmdRef, VabRef};

/// Decode TIM `t` with palette `clut` to `(width, height, rgba)`.
///
/// A 4 / 8 bpp TIM that carries no CLUT of its own (its palette is another
/// upload's VRAM cell) decodes through [`grey_ramp`], so it previews and
/// exports as its index image rather than failing.
pub fn tim_rgba(a: &Analysis, t: &TimRef, clut: usize) -> Result<(usize, usize, Vec<u8>)> {
    let tim = psx_tim::parse(a.tim_bytes(t)).context("TIM parse")?;
    let rgba = if needs_grey_ramp(&tim) {
        psx_tim::decode_rgba8_with_palette(&tim, &grey_ramp(&tim))
    } else {
        let clut = clut.min(tim.palette_count().saturating_sub(1));
        psx_tim::decode_rgba8(&tim, clut)
    }
    .context("TIM decode")?;
    Ok((tim.pixel_width(), tim.pixel_height(), rgba))
}

/// Whether `tim` is indexed but carries no CLUT.
pub fn needs_grey_ramp(tim: &psx_tim::Tim) -> bool {
    use psx_tim::PixelMode;
    matches!(tim.mode, PixelMode::Bpp4 | PixelMode::Bpp8) && tim.clut.is_none()
}

/// An opaque black-to-white BGR555 ramp as long as `tim`'s index range
/// (STP set, so index 0 is not read as transparent black).
pub fn grey_ramp(tim: &psx_tim::Tim) -> Vec<u16> {
    let n: u16 = if tim.mode == psx_tim::PixelMode::Bpp4 {
        16
    } else {
        256
    };
    (0..n)
        .map(|i| {
            let v = i * 31 / (n - 1);
            0x8000 | (v << 10) | (v << 5) | v
        })
        .collect()
}

pub fn tim_png(a: &Analysis, t: &TimRef, clut: usize, out: &Path) -> Result<()> {
    let (w, h, rgba) = tim_rgba(a, t, clut)?;
    if w == 0 || h == 0 {
        bail!("empty TIM ({w}x{h})");
    }
    psx_tim::write_png(out, w, h, &rgba)
}

/// Wavefront OBJ of the mesh, in raw PSX units (Y down), faces triangulated
/// the way the renderer splits quads.
pub fn tmd_obj(a: &Analysis, t: &TmdRef, out: &Path) -> Result<()> {
    let bytes = a.tmd_bytes(t);
    let tmd = legaia_tmd::parse(bytes).context("TMD parse")?;
    let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, bytes);
    let mut s = String::new();
    s.push_str(&format!(
        "# legaia-explorer: PROT entry {} TMD @ {} 0x{:X}\n# raw PSX units, +Y down\n",
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

/// Textured `.glb`: the mesh plus an atlas baked from `vram` - the scene's
/// VRAM image when given, else the entry's own TIMs. 1 PSX unit exports as
/// 1 cm.
pub fn tmd_glb(a: &Analysis, t: &TmdRef, vram: Option<&psx_tim::Vram>, out: &Path) -> Result<()> {
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
    let own;
    let vram = match vram {
        Some(v) => v,
        None => {
            own = a.vram();
            &own
        }
    };
    let glb = build_scene_glb(&scene_mesh.name.clone(), &[scene_mesh], &[inst], vram)
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
    psx_vab::decode_vag(body)
}

/// The width of a sample's waveform in the Audio tab.
pub const WAVE_POINTS: usize = 120;

/// `pcm` folded into [`WAVE_POINTS`] peak levels, 255 at full scale.
pub fn wave_peaks(pcm: &[i16]) -> Vec<u8> {
    if pcm.is_empty() {
        return Vec::new();
    }
    (0..WAVE_POINTS)
        .map(|p| {
            let from = p * pcm.len() / WAVE_POINTS;
            let to = ((p + 1) * pcm.len() / WAVE_POINTS).max(from + 1);
            let peak = pcm[from..to.min(pcm.len())]
                .iter()
                .map(|s| s.unsigned_abs())
                .max()
                .unwrap_or(0);
            (u32::from(peak) * 255 / 32768) as u8
        })
        .collect()
}

pub fn vag_wav(a: &Analysis, v: &VabRef, sample: usize, out: &Path) -> Result<()> {
    let pcm = vag_pcm(a, v, sample)?;
    let f = std::fs::File::create(out).with_context(|| format!("create {}", out.display()))?;
    psx_vab::write_wav(
        std::io::BufWriter::new(f),
        &pcm,
        legaia_engine_audio::vab_bind::VAB_SAMPLE_RATE,
    )?;
    Ok(())
}

/// Every sample of bank `v` as `<stem>_NNN.wav` into `dir`. Returns the count.
pub fn vab_all_wavs(a: &Analysis, v: &VabRef, dir: &Path, stem: &str) -> Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let mut failed = Vec::new();
    for i in 0..v.report.vag_samples.len() {
        if let Err(e) = vag_wav(a, v, i, &dir.join(format!("{stem}_{i:03}.wav"))) {
            failed.push(format!("#{i}: {e:#}"));
        }
    }
    Ok(failed)
}

pub fn bytes(data: &[u8], out: &Path) -> Result<()> {
    std::fs::write(out, data).with_context(|| format!("write {}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_peaks_cover_the_sample_at_a_fixed_width() {
        assert!(wave_peaks(&[]).is_empty());
        let short = wave_peaks(&[i16::MIN, 0, 16384]);
        assert_eq!(short.len(), WAVE_POINTS);
        assert_eq!(short[0], 255);
        let long: Vec<i16> = (0..10_000)
            .map(|i| if i < 5000 { 0 } else { 8192 })
            .collect();
        let p = wave_peaks(&long);
        assert_eq!((p[0], p[WAVE_POINTS - 1]), (0, 63));
    }

    #[test]
    fn grey_ramp_spans_black_to_white_and_is_opaque() {
        let mut tim = psx_tim::Tim {
            flags: 0,
            mode: psx_tim::PixelMode::Bpp4,
            clut: None,
            image: psx_tim::Image {
                fb_x: 0,
                fb_y: 0,
                fb_w: 1,
                h: 1,
                data: vec![0x10, 0xF0],
            },
        };
        assert!(needs_grey_ramp(&tim));
        let ramp = grey_ramp(&tim);
        assert_eq!(ramp.len(), 16);
        assert_eq!(ramp[0], 0x8000);
        assert_eq!(ramp[15], 0xFFFF);
        let rgba = psx_tim::decode_rgba8_with_palette(&tim, &ramp).unwrap();
        // Index 0 is opaque black, not transparent.
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        tim.mode = psx_tim::PixelMode::Bpp8;
        assert_eq!(grey_ramp(&tim).len(), 256);
        tim.mode = psx_tim::PixelMode::Bpp16;
        assert!(!needs_grey_ramp(&tim));
    }
}
