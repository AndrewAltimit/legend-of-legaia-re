//! Disc-gated: the field's lighting signal - the baked prim colour word - is
//! present, non-degenerate, and reaches the mesh the renderer draws.
//!
//! Retail runs **no light source** on the field path. Both TMD renderers
//! (`FUN_8002735c` and its light-source sibling `FUN_80029888`) issue exactly
//! one GTE colour op between them - `DPCS` (`cop2 0x780010`), the depth cue -
//! and never an `NC*` op, so no light matrix is consulted and no vertex normal
//! is transformed. The shading is baked into each primitive's colour word and
//! applied by the GPU's texture blend, `texel * colour / 128`: `0x80` is
//! neutral, below darkens, above brightens (to nearly 2x).
//!
//! That factor-of-two headroom is the whole reason retail's field has more
//! contrast than an unlit render, so the invariant worth guarding is that the
//! colour distribution has **both tails**. An engine that drops the colour (or
//! one that "normalises" it by 255 instead of 128) collapses to the raw texel
//! and silently loses them - which is exactly the bug this guards, and it is
//! invisible to any "does it decode" check because the bytes are all still
//! there.
//!
//! Also pins the two structural facts the colour block depends on:
//!   * baked-colour rows (4/5) put their colour word(s) at prim offset 0, which
//!     is what pushes their texture block to 4 (flat) / `4 * n_verts` (gouraud);
//!   * light-source-lit rows (0/1) have no colour word at all - their texture
//!     block starts at 0 - so they must come back neutral, never as the
//!     texture block reinterpreted as an RGB.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::field_env;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_resources::{
    BuildOptions, FIELD_SHARED_BLOCKS, SceneLoadKind, SceneResources,
};
use legaia_tmd::descriptor::Descriptor;
use legaia_tmd::legaia_prims::{self, MODULATION_NEUTRAL};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn env_pack_prims_carry_a_two_sided_lighting_signal() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open prot index"));

    let shared: Vec<Scene> = FIELD_SHARED_BLOCKS
        .iter()
        .filter_map(|n| Scene::load(&index, n).ok())
        .collect();
    let shared_refs: Vec<&Scene> = shared.iter().collect();
    let system_ui = index.system_ui_bundle().ok();

    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut scenes = 0usize;
    // Colour-component census over the baked-colour rows.
    let mut below = 0usize; // < 0x80 - darkens the texel
    let mut neutral = 0usize; // = 0x80 - texel unchanged
    let mut above = 0usize; // > 0x80 - BRIGHTENS the texel (up to ~2x)
    // Structural counters (keep the assertions non-vacuous).
    let mut lit_prims = 0usize;
    let mut lit_non_neutral = 0usize;
    let mut baked_prims = 0usize;
    let mut missing_colors = 0usize;
    // Every GP0 code byte seen in a colour word's 4th byte.
    let mut codes = std::collections::BTreeSet::new();

    for name in &scene_names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok((res, _)) = SceneResources::build_targeted_with_options(
            &scene,
            &shared_refs,
            BuildOptions {
                kind: SceneLoadKind::Field,
                upload_all_tims: true,
                system_ui: system_ui.as_deref(),
            },
        ) else {
            continue;
        };
        let env_tmds = field_env::env_pack_tmd_indices(&scene, &res);
        if env_tmds.is_empty() {
            continue;
        }
        scenes += 1;

        for &ti in &env_tmds {
            let t = &res.tmds[ti];
            for o in t.tmd.objects.iter() {
                let groups = legaia_prims::iter_groups_lenient(
                    &t.raw,
                    o.primitives_byte_offset,
                    o.primitives_byte_size,
                );
                for g in &groups {
                    let Some(d) = Descriptor::for_flags(g.header.flags) else {
                        continue;
                    };
                    let n_verts = d.packet_shape.n_vertices();
                    // Lit rows: texture block at offset 0 => no colour word.
                    let is_lit = d.texture_block_offset == Some(0);

                    for p in &g.prims {
                        if p.colors.len() != n_verts {
                            missing_colors += 1;
                            continue;
                        }
                        if is_lit {
                            lit_prims += 1;
                            if p.colors.iter().any(|&c| c != [MODULATION_NEUTRAL; 3]) {
                                lit_non_neutral += 1;
                            }
                            continue;
                        }
                        baked_prims += 1;

                        // The GP0 code byte lives in the 4th byte of the FIRST
                        // colour word. A gouraud packet's remaining colour words
                        // carry a zero top byte (standard PSX packet layout: only
                        // the leading word is the command).
                        if p.bytes_offset + 4 <= t.raw.len() {
                            codes.insert(t.raw[p.bytes_offset + 3]);
                        }
                        for c in &p.colors {
                            for &v in c {
                                match v.cmp(&MODULATION_NEUTRAL) {
                                    std::cmp::Ordering::Less => below += 1,
                                    std::cmp::Ordering::Equal => neutral += 1,
                                    std::cmp::Ordering::Greater => above += 1,
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let total = below + neutral + above;
    eprintln!(
        "[env-light] {scenes} scenes, {baked_prims} baked-colour prims, {lit_prims} lit prims"
    );
    eprintln!(
        "[env-light] colour components: {total} total | < 0x80 {below} ({:.1}%) | \
         = 0x80 {neutral} ({:.1}%) | > 0x80 {above} ({:.1}%)",
        100.0 * below as f64 / total as f64,
        100.0 * neutral as f64 / total as f64,
        100.0 * above as f64 / total as f64,
    );
    eprintln!("[env-light] GP0 code bytes seen: {codes:02x?}");

    assert!(scenes > 0, "no field scenes with env packs decoded");
    assert!(baked_prims > 0, "no baked-colour prims - census is vacuous");
    assert!(lit_prims > 0, "no lit-row prims - lit assertion is vacuous");
    assert_eq!(
        missing_colors, 0,
        "{missing_colors} prims decoded without one colour per vertex"
    );
    assert_eq!(
        lit_non_neutral, 0,
        "{lit_non_neutral} light-source-lit prims came back non-neutral - their texture \
         block sits at offset 0, so a colour read there would hand the renderer \
         [u0, v0, cba_lo] as an RGB"
    );

    // The leading colour word's 4th byte must be a PSX GP0 polygon command. This
    // is what proves the first three bytes really are an RGB and not geometry:
    // F3 0x20 / FT3 0x24 / F4 0x28 / FT4 0x2C / G3 0x30 / GT3 0x34 / G4 0x38 /
    // GT4 0x3C, each optionally `| 2` for the semi-transparent (ABE) variant.
    // Every one of the eight shows up across the corpus.
    for &c in &codes {
        let base = c & !0x02;
        assert!(
            matches!(base, 0x20 | 0x24 | 0x28 | 0x2C | 0x30 | 0x34 | 0x38 | 0x3C),
            "leading colour word's 4th byte {c:#04x} is not a GP0 polygon command - \
             the colour block is not where we think it is"
        );
    }
    for base in [0x20u8, 0x24, 0x28, 0x2C, 0x30, 0x34, 0x38, 0x3C] {
        assert!(
            codes.contains(&base) || codes.contains(&(base | 0x02)),
            "GP0 command {base:#04x} never seen - the code-byte check is weaker than it looks"
        );
    }

    // The signal must have BOTH tails: colours that darken the texel and colours
    // that brighten it. Dropping the modulation (or dividing by 255 instead of
    // 128) throws the brightening tail away and flattens the scene's contrast.
    let pct = |n: usize| 100.0 * n as f64 / total as f64;
    assert!(
        pct(below) > 50.0,
        "expected most colour components below neutral (they darken); got {:.1}%",
        pct(below)
    );
    assert!(
        pct(above) > 5.0,
        "expected a real brightening tail above neutral; got {:.1}% - if this \
         collapses, the >0x80 headroom (up to ~2x) that gives retail its contrast \
         is gone",
        pct(above)
    );
    // ...and the brightening really does reach toward the 2x ceiling.
    assert!(
        pct(neutral) > 1.0,
        "expected a neutral population too; got {:.1}%",
        pct(neutral)
    );
}
