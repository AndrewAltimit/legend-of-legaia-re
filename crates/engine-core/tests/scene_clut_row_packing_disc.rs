//! Disc-gated: a **densely packed CLUT row does not disqualify a prim**.
//!
//! A 4bpp primitive's CBA addresses one of 64 distinct 16-entry palettes on
//! a VRAM row (`(cba & 0x3F) * 16` spans `0..=1008`), so a scene that packs
//! many palettes onto one row leaves it populated across most of its 1024
//! pixels. The GPU still reads only the 16 entries at the prim's own CBA.
//!
//! The VRAM-coverage filter used to reject a 4bpp prim whose CLUT scanline
//! ran past 256 populated pixels, on the theory that a wide row meant
//! another TIM's image data had spilled onto it. That bound was a quarter of
//! what the encoding permits, so it fired on ordinary scenes: in `town0b`
//! every primitive of the placed house at tile `(37, 42)` sampled the
//! densely-packed row 491 and was dropped, leaving a hole through to the
//! clear colour where the building should stand. See
//! `legaia_tim::vram::PrimTextureStatus::ClutDepthMismatch`.
//!
//! Both halves are asserted, because either alone can pass vacuously: the
//! house must keep its prims (a filter that dropped everything would still
//! satisfy a "no depth mismatches" assertion), and the row it samples must
//! really be packed wider than the old bound (otherwise this scene never
//! exercised the bug and the test proves nothing).
//!
//! Assertions are structural - prim counts and VRAM occupancy. No Sony
//! bytes. Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

const SCENE: &str = "town0b";
/// The placed house at tile `(37, 42)` - env-pack slot of placement 22.
const HOUSE_PACK_SLOT: usize = 42;
/// The CLUT row its primitives sample.
const HOUSE_CLUT_ROW: usize = 491;
/// The width the removed heuristic treated as the maximum legitimate run
/// for a 4bpp palette row.
const OLD_WIDTH_BOUND: usize = 256;

fn open_host() -> Option<SceneHost> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    None
}

#[test]
fn a_densely_packed_clut_row_keeps_its_prims() {
    let Some(mut host) = open_host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ missing");
        return;
    };
    if host.enter_field_scene(SCENE, 0).is_err() {
        eprintln!("[skip] {SCENE} did not load");
        return;
    }
    let (Some(res), Some(scene)) = (host.resources.as_ref(), host.scene.as_ref()) else {
        eprintln!("[skip] {SCENE} resources missing");
        return;
    };
    let env_tmds = legaia_engine_core::field_env::env_pack_tmd_indices(scene, res);

    // --- The VRAM half: the row really is packed past the old bound. ---
    let packed = (0..legaia_tim::vram::VRAM_WIDTH)
        .filter(|&x| res.vram.pixel(x, HOUSE_CLUT_ROW) != 0)
        .count();
    assert!(
        packed > OLD_WIDTH_BOUND,
        "{SCENE} CLUT row {HOUSE_CLUT_ROW} holds only {packed} populated px, so it \
         never exercised the width bound of {OLD_WIDTH_BOUND} - this test is vacuous"
    );

    // --- The mesh half: the house survives the filter. ---
    let slot = env_tmds
        .get(HOUSE_PACK_SLOT)
        .and_then(|&t| res.tmds.get(t))
        .expect("env pack slot 42 resolves");
    let (mesh, reasons) = slot.build_filtered_vram_mesh_reasoned(&res.vram);
    assert!(
        reasons.kept > 100,
        "the {SCENE} house should keep its prims, got {reasons:?}"
    );
    assert!(
        !mesh.indices.is_empty(),
        "the {SCENE} house must build a non-empty mesh"
    );
    assert_eq!(
        reasons.clut_depth_mismatch, 0,
        "no prim may be dropped for a CLUT-row width; width cannot decide this"
    );

    // Scene-wide: nothing anywhere is dropped on the retired reason.
    for (n, &t) in env_tmds.iter().enumerate() {
        let Some(rtmd) = res.tmds.get(t) else {
            continue;
        };
        let (_, r) = rtmd.build_filtered_vram_mesh_reasoned(&res.vram);
        assert_eq!(
            r.clut_depth_mismatch, 0,
            "{SCENE} pack[{n}] dropped {} prim(s) on CLUT-row width",
            r.clut_depth_mismatch
        );
    }
}
