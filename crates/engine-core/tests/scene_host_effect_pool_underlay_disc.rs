//! Disc-gated: the engine host's field entry layers the effect-texture pool
//! **under** its scene VRAM, and seats the fog pool's cap from the scene MAN.
//!
//! The pool (PROT 0874 section 2) is where the field fog sheets sample:
//! texpage `0x0027` = VRAM `(448, 0)`, rows `v 0x40..0x6F`, CLUT `0x7640` =
//! `(0, 473)`. Retail uploads it before the scene, so a scene TIM on an
//! overlapping rect keeps its own texels - `dolk`'s capture holds the scene's
//! texels on all 1010 halfwords of the `(448, 0)` page where the two differ.
//! `SceneHost::enter_field_scene` used to write the pool over its build, which
//! is the VRAM the browser play page draws from. The two FNV-1a hashes are the
//! retail ones `engine-shell`'s `window/fog_texture_tests.rs` pins (nine
//! PCSX-Redux field / world-map states agree).
//!
//! The cap: `FUN_8003AEB0` writes `_DAT_8007B6A8 = MAN[1] & 1` at `0x8003AF54`,
//! then at `0x8003B6BC..0x8003B6E8` stores `0x48` into `_DAT_8007BCB0` when
//! `MAN[1] & 4` or that byte is set, `0x18` otherwise. The census below prints
//! the scenes whose MAN raises either bit.
//!
//! "Under" means under what no scene upload **wrote**, not under every zero
//! word: a caption TIM's transparent background is written zeros, and the
//! ending scenes' credit captions at `(320, 416)` sit on the pool's `(320,
//! 256..)` kanji sheet - keyed on the value, the kanji bled through every
//! transparent run of the credits roll.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / `extracted/` are missing
//! (`LEGAIA_EXTRACTED_DIR` first, then repo-relative).

use legaia_engine_core::fog_particles::{FOG_CAP_DEFAULT, FOG_CAP_RAISED, fog_cap_for_man};
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use std::path::PathBuf;

const RETAIL_FOG_CELLS_FNV: u64 = 0xe9d8_110d_1eec_6070;
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
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = ["extracted", "../extracted", "../../extracted"].map(PathBuf::from);
    for d in env.into_iter().chain(rel) {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

fn host_vram(extracted: &std::path::Path, scene: &str) -> SceneHost {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.enter_field_scene(scene, 0).expect("enter field scene");
    host
}

#[test]
fn host_field_entry_underlays_the_effect_pool() {
    let Some(extracted) = extracted() else { return };
    let index = ProtIndex::open_extracted(&extracted).expect("index");
    let mut pool = legaia_tim::Vram::new();
    legaia_engine_core::scene::upload_effect_textures_into_vram(&index, &mut pool, true)
        .expect("effect pool");

    // dolk: the scene's own texels win on the (448, 0) page.
    let host = host_vram(&extracted, "dolk");
    let vram = &host.resources.as_ref().expect("dolk resources").vram;
    let differing = (0..256)
        .flat_map(|y| (448..512).map(move |x| (x, y)))
        .filter(|&(x, y)| vram.pixel(x, y) != pool.pixel(x, y))
        .count();
    eprintln!("[ok] dolk (448, 0) page: {differing} halfwords keep the scene's texels");
    assert!(
        differing >= 1010,
        "dolk's scene TIMs were overwritten by the effect pool ({differing} differ)"
    );

    // vell (and dolk's CLUT row): the fog cells + CLUT are retail's.
    for scene in ["vell", "town01"] {
        let host = host_vram(&extracted, scene);
        let vram = &host.resources.as_ref().expect("resources").vram;
        let cells = fnv((0x40..0x70)
            .flat_map(|y| (448..464).map(move |x| (x, y)))
            .map(|(x, y)| vram.pixel(x, y)));
        let clut = fnv((0..16).map(|x| vram.pixel(x, 473)));
        eprintln!("[ok] {scene}: fog cells {cells:016x}, clut {clut:016x}");
        assert_eq!(cells, RETAIL_FOG_CELLS_FNV, "{scene}: fog wisps");
        assert_eq!(clut, RETAIL_FOG_CLUT_FNV, "{scene}: fog CLUT");
    }
}

/// Every word of `edteien`'s `(320, 416)` 40x32 caption TIM - the credits
/// roll's first card - lands as the TIM wrote it, transparent zeros
/// included, although the effect pool covers the same rect.
#[test]
fn a_caption_tims_transparent_words_stay_transparent_over_the_pool() {
    let Some(extracted) = extracted() else { return };
    let index = ProtIndex::open_extracted(&extracted).expect("index");
    let mut pool = legaia_tim::Vram::new();
    legaia_engine_core::scene::upload_effect_textures_into_vram(&index, &mut pool, true)
        .expect("effect pool");
    let scene = Scene::load(&index, "edteien").expect("edteien");
    let mut caption = None;
    for entry in &scene.entries {
        let bytes: &[u8] = &entry.bytes;
        let scan = legaia_asset::tim_scan::scan_entry(bytes);
        for (source, hit) in &scan.hits {
            let src: &[u8] = match source {
                legaia_asset::tim_scan::Source::Raw => bytes,
                legaia_asset::tim_scan::Source::Lzs(i) => scan.lzs_sections[*i].as_slice(),
            };
            let end = (hit.offset + hit.byte_len).min(src.len());
            if let Ok(t) = legaia_tim::parse(&src[hit.offset..end])
                && (t.image.fb_x, t.image.fb_y, t.image.fb_w, t.image.h) == (320, 416, 40, 32)
            {
                caption = Some(t);
            }
        }
    }
    let caption = caption.expect("edteien carries its (320, 416) 40x32 caption TIM");
    let host = host_vram(&extracted, "edteien");
    let vram = &host.resources.as_ref().expect("edteien resources").vram;
    let (mut zeros, mut pool_under_zero, mut mismatched) = (0usize, 0usize, 0usize);
    for row in 0..32usize {
        for col in 0..40usize {
            let o = (row * 40 + col) * 2;
            let want = u16::from_le_bytes([caption.image.data[o], caption.image.data[o + 1]]);
            let (x, y) = (320 + col, 416 + row);
            if want == 0 {
                zeros += 1;
                if pool.pixel(x, y) != 0 {
                    pool_under_zero += 1;
                }
            }
            if vram.pixel(x, y) != want {
                mismatched += 1;
            }
        }
    }
    eprintln!(
        "[ok] edteien caption: {zeros} transparent words, {pool_under_zero} of them over pool texels, {mismatched} mismatched"
    );
    assert!(
        pool_under_zero > 0,
        "the pool must actually cover the caption for this to test anything"
    );
    assert_eq!(
        mismatched, 0,
        "pool texels bled into the caption's transparent words"
    );
}

#[test]
fn fog_cap_follows_the_scene_man_flags() {
    let Some(extracted) = extracted() else { return };
    let index = ProtIndex::open_extracted(&extracted).expect("index");
    let mut raised = Vec::new();
    let mut total = 0usize;
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        total += 1;
        if fog_cap_for_man(&man) == FOG_CAP_RAISED {
            raised.push(format!("{name}(MAN[1]={:#04x})", man[1]));
        }
    }
    eprintln!(
        "[ok] {} of {total} scene MANs raise the fog cap to 0x48: {}",
        raised.len(),
        raised.join(" ")
    );
    assert!(total > 50, "census found only {total} scene MANs");

    // Live: an ordinary field scene seats 0x18, a kingdom overworld 0x48 -
    // the value every PCSX-Redux map01 / map03 state holds at 0x8007BCB0.
    let host = host_vram(&extracted, "vell");
    assert_eq!(host.world.fog.cap, FOG_CAP_DEFAULT, "vell cap");
    let man = Scene::load(&index, "map01")
        .expect("map01")
        .field_man_payload(&index)
        .expect("map01 MAN")
        .expect("map01 carries a MAN");
    assert_eq!(fog_cap_for_man(&man), FOG_CAP_RAISED, "map01 cap");
}
