//! Scene and stage luminance measured off the textured meshes, and the fog colour scale it drives.

/// How bright the bank may be relative to the scene's measured luminance
/// `L`: at most `FOG_LUMA_FLOOR + FOG_LUMA_OVER_SCENE * L`. The floor keeps a
/// night bank readable as a moonlit haze against a dark stage (a mist reads
/// by contrast, it does not vanish); the slope keeps it below a lit scene's
/// brightness.
pub const FOG_LUMA_FLOOR: f32 = 0.17;
pub const FOG_LUMA_OVER_SCENE: f32 = 1.8;

/// Rec. 601 luma of a framebuffer-space colour.
pub fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// The factor (`<= 1`) that dims `color` to the brightness a scene of
/// luminance `scene_luma` allows ([`FOG_LUMA_FLOOR`],
/// [`FOG_LUMA_OVER_SCENE`]).
pub fn luma_scale(color: [f32; 3], scene_luma: f32) -> f32 {
    let l = luma(color);
    if l <= 1.0e-6 {
        return 1.0;
    }
    ((FOG_LUMA_FLOOR + scene_luma.max(0.0) * FOG_LUMA_OVER_SCENE) / l).min(1.0)
}

/// The scene's luminance as the player sees it, measured from its meshes:
/// every textured triangle's texel at its UV centroid (through its CLUT),
/// modulated by the prim's baked colour word (`texel * colour / 128`, the
/// field shading), weighted by the triangle's world area and averaged.
/// Transparent texels (the zero word) are skipped. `None` when nothing
/// textured was found. Deterministic; both hosts call it on the same
/// meshes (the field scene's pool at entry, the battle stage dome at
/// battle entry).
pub fn scene_luminance<'a>(
    vram: &legaia_tim::Vram,
    meshes: impl IntoIterator<Item = &'a legaia_tmd::mesh::VramMesh>,
) -> Option<f32> {
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for m in meshes {
        for tri in m.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| i as usize);
            if a >= m.positions.len() || b >= m.positions.len() || c >= m.positions.len() {
                continue;
            }
            let (pa, pb, pc) = (m.positions[a], m.positions[b], m.positions[c]);
            let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
            let cr = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let area = 0.5 * ((cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]) as f64).sqrt();
            if area <= 0.0 {
                continue;
            }
            let u = (u32::from(m.uvs[a][0]) + u32::from(m.uvs[b][0]) + u32::from(m.uvs[c][0])) / 3;
            let v = (u32::from(m.uvs[a][1]) + u32::from(m.uvs[b][1]) + u32::from(m.uvs[c][1])) / 3;
            let [cba, tsb] = m.cba_tsb[a];
            let Some(word) = texel_word(vram, cba, tsb, u as usize, v as usize) else {
                continue;
            };
            let rgb = [
                f32::from(word & 31) / 31.0,
                f32::from((word >> 5) & 31) / 31.0,
                f32::from((word >> 10) & 31) / 31.0,
            ];
            let col = m.colors.get(a).copied().unwrap_or([0x80; 3]);
            let lit = [
                (rgb[0] * f32::from(col[0]) / 128.0).min(1.0),
                (rgb[1] * f32::from(col[1]) / 128.0).min(1.0),
                (rgb[2] * f32::from(col[2]) / 128.0).min(1.0),
            ];
            sum += f64::from(luma(lit)) * area;
            weight += area;
        }
    }
    (weight > 0.0).then(|| (sum / weight) as f32)
}

/// [`scene_luminance`] of a battle stage shell as drawn: the backdrop TMD's
/// drawn objects (`SceneHost::battle_stage_object_indices`), textured prims,
/// over the battle VRAM. The one measurement both hosts take at battle entry
/// and store in [`FogVolume::battle_luma`]; the shell's second copy repeats
/// the first, so it is not measured twice.
pub fn stage_luminance(
    vram: &legaia_tim::Vram,
    tmd: &legaia_tmd::Tmd,
    raw: &[u8],
    objects: &[usize],
) -> Option<f32> {
    let shell = legaia_scene_formats::battle_backdrop::objects_tmd(tmd, objects);
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&shell, raw);
    scene_luminance(vram, [&mesh])
}

/// The 15-bit word a textured prim samples at `(u, v)`, through its CLUT
/// for the indexed depths; `None` for the transparent zero word.
fn texel_word(vram: &legaia_tim::Vram, cba: u16, tsb: u16, u: usize, v: usize) -> Option<u16> {
    let cx = usize::from(cba & 0x3F) * 16;
    let cy = usize::from((cba >> 6) & 0x1FF);
    let tx = usize::from(tsb & 0xF) * 64;
    let ty = usize::from((tsb >> 4) & 1) * 256;
    let px = |x: usize, y: usize| -> u16 {
        if x >= legaia_tim::VRAM_WIDTH || y >= legaia_tim::VRAM_HEIGHT {
            0
        } else {
            vram.pixel(x, y)
        }
    };
    let w = match (tsb >> 7) & 3 {
        0 => {
            let i = (px(tx + (u >> 2), ty + v) >> ((u & 3) * 4)) & 0xF;
            px(cx + usize::from(i), cy)
        }
        1 => {
            let i = (px(tx + (u >> 1), ty + v) >> ((u & 1) * 8)) & 0xFF;
            px(cx + usize::from(i), cy)
        }
        _ => px(tx + u, ty + v),
    };
    (w != 0).then_some(w)
}
