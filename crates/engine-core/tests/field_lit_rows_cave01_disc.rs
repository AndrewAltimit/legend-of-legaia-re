//! `cave01`'s rock walls are light-source rows, and the shading kernel turns
//! the wall facing away from the scene-load light dark.
//!
//! Every terrain draw of the cave's corridor (packs 29 and 37) carries group
//! flags `0x11` - kind 8, `NCCS` - so retail colours each face from its
//! normal. The `cave01_attached_light` retail frame shows pack 37's column at
//! about an eighth of its texel and pack 29's above neutral; drawing the lit
//! rows at the neutral `0x80` (the mesh builder's fill) painted both at their
//! raw texel. This pins the mesh builder's normals (unit length, read from the
//! packet's normal index) and the shading's split between the two columns.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::field_lit_mesh::{draw_rotation, has_lit_rows, shade_lit_rows};
use legaia_engine_core::scene::ProtIndex;
use legaia_engine_core::scene_assembly::assemble_field_scene;
use legaia_engine_vm::field_light::FieldLight;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Mean shaded colour of one pack's lit vertices, drawn unrotated.
fn mean_lit_shade(a: &legaia_engine_core::scene_assembly::AssembledScene, slot: usize) -> f32 {
    let res_idx = a.env_tmds[slot];
    let rtmd = &a.res.tmds[res_idx];
    let (mesh, lit) = rtmd.build_filtered_vram_mesh_lit_vertices(&a.res.vram);
    assert!(has_lit_rows(&lit), "pack {slot} carries light-source rows");
    for v in lit.iter().flatten() {
        let n = v.normal.map(|c| f32::from(c) / 4096.0);
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!(
            (len - 1.0).abs() < 0.02,
            "pack {slot} normal {n:?} is unit length"
        );
    }
    let mut colors = mesh.colors.clone();
    shade_lit_rows(
        &mut colors,
        &lit,
        &FieldLight::SCENE_LOAD,
        &draw_rotation(0, 0, 0),
    );
    let shaded: Vec<f32> = colors
        .iter()
        .zip(&lit)
        .filter(|(_, l)| l.is_some())
        .map(|(c, _)| f32::from(c[0]))
        .collect();
    shaded.iter().sum::<f32>() / shaded.len() as f32
}

#[test]
fn cave01_walls_shade_by_the_scene_load_light_or_skip() {
    let Some(root) = extracted_root() else { return };
    let index = ProtIndex::open_extracted(&root).expect("prot index");
    let a = assemble_field_scene(&index, "cave01").expect("assemble cave01");
    let used: std::collections::BTreeSet<usize> = a.terrain.iter().map(|d| d.env_slot).collect();
    assert!(
        used.contains(&29) && used.contains(&37),
        "corridor packs drawn"
    );
    let left = mean_lit_shade(&a, 29);
    let right = mean_lit_shade(&a, 37);
    println!("cave01 mean lit shade: pack 29 {left:.1}, pack 37 {right:.1}");
    // Mean over every lit corner (tops and returns included), so the split is
    // relative: the light-facing column averages over twice the other.
    assert!(left > 0x50 as f32, "the left wall faces the light ({left})");
    assert!(
        right * 2.0 < left,
        "the right wall turns from it ({right} vs {left})"
    );
}
