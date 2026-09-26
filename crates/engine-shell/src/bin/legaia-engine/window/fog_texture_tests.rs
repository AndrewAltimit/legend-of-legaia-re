//! Disc-gated: the native window's scene VRAM holds the texels the field fog
//! sheets sample, byte-for-byte what retail holds there.
//!
//! The fog pass (`FUN_8003F86C`) links `POLY_FT4`s on texture page `0x0027`
//! (VRAM `(448, 0)`, 4bpp, ABR 1) through CLUT `0x7640` = `(0, 473)`,
//! sampling rows `v 0x40..0x6F` (the staged UV rows at `0x8007322C`). Both
//! live in the effect-texture pool of PROT 0874 section 2, which retail
//! keeps resident in field and world-map VRAM. The window built its scene
//! VRAM without that pool, so every fog fragment sampled a zero word and the
//! overlay shader discarded it: the pool was live, the quads were emitted,
//! and nothing reached the frame.
//!
//! The two hashes below are FNV-1a over the halfwords (row-major), measured
//! on nine PCSX-Redux field / world-map states (`retona`, `town01`,
//! `chitei2`, `dolk`, `map01`, `son`, `vozz`, `map03`, `vell`) with
//! `scripts/pcsx-redux/extract_vram_from_sstate.py`; all nine agree.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use super::run::build_window_scene_resources;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use std::path::PathBuf;

/// Retail hash of VRAM `x 448..464, y 0x40..0x70` - the fog wisps.
const RETAIL_FOG_CELLS_FNV: u64 = 0xe9d8_110d_1eec_6070;
/// Retail hash of VRAM `x 0..16, y 473` - CLUT `0x7640`.
const RETAIL_FOG_CLUT_FNV: u64 = 0x3e91_7d19_3b0e_dcf5;

fn fnv(vals: impl Iterator<Item = u16>) -> u64 {
    vals.fold(0xcbf2_9ce4_8422_2325u64, |h, p| {
        (h ^ u64::from(p)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn extracted() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

fn window_vram(extracted: &std::path::Path, scene: &str) -> legaia_tim::Vram {
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).expect("boot session");
    session
        .enter_field_live(scene, &FieldLiveOpts::default())
        .expect("enter field scene");
    build_window_scene_resources(&session)
        .expect("window scene resources")
        .vram
}

#[test]
fn window_vram_holds_the_retail_fog_texels() {
    let Some(extracted) = extracted() else { return };
    for scene in [
        "map01", "map03", "vell", "retona", "town01", "chitei2", "dolk", "son", "vozz",
    ] {
        let vram = window_vram(&extracted, scene);
        let cells = fnv((0x40..0x70)
            .flat_map(|y| (448..464).map(move |x| (x, y)))
            .map(|(x, y)| vram.pixel(x, y)));
        let clut = fnv((0..16).map(|x| vram.pixel(x, 473)));
        eprintln!("[ok] {scene}: fog cells {cells:016x}, clut {clut:016x}");
        assert_eq!(
            cells, RETAIL_FOG_CELLS_FNV,
            "{scene}: fog wisps at (448, 64..112)"
        );
        assert_eq!(clut, RETAIL_FOG_CLUT_FNV, "{scene}: fog CLUT at (0, 473)");
    }
}

/// The pool goes **under** the scene build, not over it: retail loads it
/// before the scene, so a scene TIM on an overlapping rect keeps its own
/// texels. `dolk` is the field scene whose TIMs reach into the pool's
/// `(448, 0)` page; its capture holds the scene's texels on every one of
/// the 1010 halfwords where the two differ.
#[test]
fn scene_texels_win_over_the_pool() {
    let Some(extracted) = extracted() else { return };
    let vram = window_vram(&extracted, "dolk");
    let index = legaia_engine_core::scene::ProtIndex::open_extracted(&extracted).expect("index");
    let mut pool = legaia_tim::Vram::new();
    legaia_engine_core::scene::upload_effect_textures_into_vram(&index, &mut pool, true)
        .expect("effect pool");
    let differing = (0..256)
        .flat_map(|y| (448..512).map(move |x| (x, y)))
        .filter(|&(x, y)| vram.pixel(x, y) != pool.pixel(x, y))
        .count();
    eprintln!("[ok] dolk (448, 0) page: {differing} halfwords keep the scene's texels");
    assert!(
        differing >= 1010,
        "dolk's scene TIM was overwritten by the pool"
    );
}
