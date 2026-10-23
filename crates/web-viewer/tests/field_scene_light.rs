//! Disc-gated: the map viewer's light-source rows shade under the light the
//! scene itself sets, not the scene loader's reset.
//!
//! Every Rim Elm variant's scene-entry record `P1[0]` issues field-VM op
//! `4C 8A C4 09 C4 09 24 FA FF FF FF` - angles `(0x9C4, 0x9C4, -0x5DC)` and
//! a white back colour. Retail RAM at `town0c` holds exactly that at
//! `_DAT_8007B780..88` (three captured town0c save states agree). The
//! viewer builds its lit copies (`FieldScenePack::lit_env_mesh`) before the
//! live scene's first tick, under `FieldLight::SCENE_LOAD`'s `0x202020`
//! back colour, which leaves a face turned from the light at an eighth of
//! its texel: Rim Elm's round huts (env slot 36) drew dark until the page
//! re-shaded them on the light's change.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_scene::scene::ProtIndex;
use legaia_iso::image::{extract_cdname_txt, extract_prot_dat};
use legaia_render_kernels::field_light::FieldLight;
use legaia_web_viewer::field_scene::{
    build_field_scene, build_field_scene_live, tick_field_scene_vsync,
};
use std::env;
use std::fs;
use std::sync::Arc;

/// `_DAT_8007B780..88` in the retail town0c captures.
const RIM_ELM_LIGHT: FieldLight = FieldLight {
    angles: [0x9C4, 0x9C4, -0x5DC],
    back: [0xFF, 0xFF, 0xFF],
};

/// The round hut's env slot and the yaw of one of its two placements.
const HUT_SLOT: usize = 36;
const HUT_ROT: [u16; 3] = [0, 3584, 0];

fn index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex"))
}

fn luma(colors: &[[u8; 3]]) -> u64 {
    colors.iter().flatten().map(|&c| u64::from(c)).sum()
}

#[test]
fn rim_elm_lit_rows_shade_under_the_scene_light_or_skip() {
    let Some(index) = index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let index = Arc::new(index);
    for scene in ["town01", "town0b", "town0c"] {
        let mut pack = build_field_scene(&index, scene).expect("build scene");
        build_field_scene_live(index.clone(), &mut pack);
        assert!(pack.live.is_some(), "{scene} runs live");
        assert_eq!(
            pack.field_light(),
            FieldLight::SCENE_LOAD,
            "{scene} pre-tick"
        );
        let (before, _, has_lit) = pack.lit_env_mesh(HUT_SLOT, 0, HUT_ROT).expect("hut slot");
        assert!(has_lit, "{scene}: the round hut carries light-source rows");

        // The scene's own 4C 8A lands on its first game tick.
        let mut vsyncs = 0;
        while pack.field_light() == FieldLight::SCENE_LOAD && vsyncs < 8 {
            tick_field_scene_vsync(&mut pack);
            vsyncs += 1;
        }
        assert_eq!(
            pack.field_light(),
            RIM_ELM_LIGHT,
            "{scene} after {vsyncs} vsyncs"
        );

        let (after, _, _) = pack.lit_env_mesh(HUT_SLOT, 0, HUT_ROT).unwrap();
        assert_eq!(before.colors.len(), after.colors.len());
        // A white back colour pins every lit corner at or above where the
        // loader's light left it; the faces turned from the light lift most.
        for (b, a) in before.colors.iter().zip(&after.colors) {
            for ch in 0..3 {
                assert!(a[ch] >= b[ch], "{scene}: {a:?} darker than {b:?}");
            }
        }
        let (lb, la) = (luma(&before.colors), luma(&after.colors));
        assert!(
            la > lb + lb / 10,
            "{scene}: scene light lifts the hut ({lb} -> {la})"
        );
    }
}
