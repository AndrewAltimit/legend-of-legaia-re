//! The assembled field-scene meshes must carry each prim's PSX
//! semi-transparency (ABE) state through to the WebGL draw stream - the
//! fountain water in Hunter's Spring (`izumi`) and the window light shafts in
//! Rim Elm's houses (`town01`) are ABE prims that read as opaque grey blobs
//! if the blend word is dropped or ignored. The site renderer keys its blend
//! pass off TSB bit 15 (`TSB_SEMI_TRANSPARENT_BIT`) + ABR bits 5..=6 of the
//! per-vertex `cba_tsb` attribute, exactly what this test pins.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc
//! data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_tim::Vram;
use legaia_tmd::mesh::{TSB_SEMI_TRANSPARENT_BIT, VramMesh};
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_scene::{build_field_scene, build_hybrid_env_mesh};
use std::env;
use std::fs;

fn load_index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex"))
}

/// Decode the BGR555 texel a vertex samples, replicating the fragment
/// shader's page + CLUT lookup (4/8bpp indirection, 15bpp direct).
fn sample_texel(vram: &Vram, uv: [u8; 2], cba: u16, tsb: u16) -> u16 {
    let (u, v) = (uv[0] as u32, uv[1] as u32);
    let tpage_x = ((tsb & 15) as u32) * 64;
    let tpage_y = (((tsb >> 4) & 1) as u32) * 256;
    let clut = |idx: u16| {
        vram.pixel(
            ((cba & 63) * 16 + idx) as usize,
            ((cba >> 6) & 511) as usize,
        )
    };
    match (tsb >> 7) & 3 {
        0 => {
            let word = vram.pixel((tpage_x + (u >> 2)) as usize, (tpage_y + v) as usize);
            clut((word >> ((u & 3) * 4)) & 15)
        }
        1 => {
            let word = vram.pixel((tpage_x + (u >> 1)) as usize, (tpage_y + v) as usize);
            clut((word >> ((u & 1) * 8)) & 255)
        }
        _ => vram.pixel((tpage_x + u) as usize, (tpage_y + v) as usize),
    }
}

/// Per-scene tally of the semi-transparent triangles in one assembled mesh.
#[derive(Default)]
struct SemiStats {
    tris: usize,
    mode_hist: [usize; 4],
    /// Corner texels of semi prims whose STP bit is set (the per-texel
    /// blend gate the shader honours).
    stp_texels: usize,
    corner_texels: usize,
}

fn tally(mesh: &VramMesh, flat: &[u8], vram: &Vram, stats: &mut SemiStats) {
    for tri in mesh.indices.chunks_exact(3) {
        let v0 = tri[0] as usize;
        let tsb = mesh.cba_tsb[v0][1];
        if tsb & TSB_SEMI_TRANSPARENT_BIT == 0 {
            continue;
        }
        stats.tris += 1;
        stats.mode_hist[((tsb >> 5) & 3) as usize] += 1;
        // Untextured verts (flat flag 0) have no texels to sample.
        let untextured = flat.get(v0 * 4 + 3).map(|&f| f == 0).unwrap_or(false);
        if untextured {
            continue;
        }
        for &vi in tri {
            let v = vi as usize;
            let raw = sample_texel(vram, mesh.uvs[v], mesh.cba_tsb[v][0], mesh.cba_tsb[v][1]);
            if raw != 0 {
                stats.corner_texels += 1;
                if raw & 0x8000 != 0 {
                    stats.stp_texels += 1;
                }
            }
        }
    }
}

#[test]
fn assembled_scene_meshes_keep_semi_transparent_prims() {
    let Some(index) = load_index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping field-scene blend test");
        return;
    };

    for name in ["izumi", "town01"] {
        let pack = build_field_scene(&index, name)
            .unwrap_or_else(|e| panic!("{name}: build_field_scene failed: {e}"));
        let referenced: std::collections::BTreeSet<usize> = pack
            .placements
            .iter()
            .chain(pack.terrain.iter())
            .map(|d| d.env_slot)
            .collect();

        let mut stats = SemiStats::default();
        for &slot in &referenced {
            let rtmd = &pack.res.tmds[pack.env_tmds[slot]];
            let (mesh, flat) = build_hybrid_env_mesh(rtmd, &pack.res.vram);
            tally(&mesh, &flat, &pack.res.vram, &mut stats);
        }
        eprintln!(
            "{name}: {} semi tris, ABR modes {:?}, STP texels {}/{}",
            stats.tris, stats.mode_hist, stats.stp_texels, stats.corner_texels
        );
        assert!(
            stats.tris > 0,
            "{name}: no semi-transparent prims survived into the draw stream"
        );
        // The blend prims' texels overwhelmingly carry the STP bit - the
        // per-texel gate that makes them blend rather than draw opaque. If
        // this drops to zero the water/light prims silently go opaque again.
        assert!(
            stats.stp_texels * 2 > stats.corner_texels,
            "{name}: semi prims' texels lost their STP blend gate \
             ({}/{} STP)",
            stats.stp_texels,
            stats.corner_texels
        );
    }
}

/// Hunter's Spring specifically: the fountain water. The big basin's water
/// disc is an ABR-mode-3 (`B + 0.25F`) textured sheet; the small fountain
/// blends at mode 0 (`0.5B + 0.5F`). Both populations must survive assembly
/// - this is the "grey fountain disc" regression.
#[test]
fn izumi_fountain_water_blend_modes() {
    let Some(index) = load_index() else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping izumi fountain blend test");
        return;
    };
    let pack = build_field_scene(&index, "izumi").expect("izumi build");
    let referenced: std::collections::BTreeSet<usize> = pack
        .placements
        .iter()
        .chain(pack.terrain.iter())
        .map(|d| d.env_slot)
        .collect();
    let mut stats = SemiStats::default();
    for &slot in &referenced {
        let rtmd = &pack.res.tmds[pack.env_tmds[slot]];
        let (mesh, flat) = build_hybrid_env_mesh(rtmd, &pack.res.vram);
        tally(&mesh, &flat, &pack.res.vram, &mut stats);
    }
    assert!(
        stats.mode_hist[3] >= 100,
        "izumi: big-fountain mode-3 water population missing: {:?}",
        stats.mode_hist
    );
    assert!(
        stats.mode_hist[0] >= 16,
        "izumi: small-fountain mode-0 water population missing: {:?}",
        stats.mode_hist
    );
}
