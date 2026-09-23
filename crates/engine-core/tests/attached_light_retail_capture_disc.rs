//! Retail capture of an op `0x34` sub-1 **attached light** - the `dolk`
//! darkness mask - read out of a catalogued PCSX-Redux state and set beside
//! the engine's own spawn of the same op.
//!
//! The state is `drake_castle_to_worldmap` (`scripts/scenarios.toml`, retail
//! at every patcher site): field mode in `dolk`, the player at world
//! `(6336, 5872)`. Its field actor lists hold one actor ticked by
//! `FUN_801E4470` whose `+0x90` is the player, and the frame's ordering-table
//! packets hold the light pool `FUN_801E3984` built from it. Two things are
//! pinned:
//!
//! 1. **The record.** Retail's `+0x3C/+0x3E` extents, `+0x74` / `+0x88`
//!    colours, `+0x5A` blend and `+0x14..+0x18` offset equal the engine's
//!    `FieldAttachedLight` after `enter_field_scene("dolk")` - the op decode
//!    and the colour packing (`R << 16 | G << 8 | B`, the byte order
//!    `FUN_801E3984` stores into the packet at `sb 4/5/6`) are exact.
//! 2. **The projection, retail side.** `FUN_801E4470` hands the extents to
//!    `FUN_800195A8`, which adds them to the parent point **after** the
//!    MVMVA through the field view matrix (`FUN_8003D344`), then projects
//!    the four corners through an identity rotation (`FUN_8003D178`). The
//!    field view matrix carries the base-matrix scale `_DAT_8007BF10`
//!    (`24576 * I`, six times world scale - `renderer.md`), so the extents
//!    are **view-space** units: the rim radius on screen is
//!    `H * ext / vz`, with `vz` the eye depth in the scaled space. The
//!    test recomputes that from the state's own scratchpad view matrix
//!    (`0x1F8003C8`) and matches the rim the packets carry.
//!
//! The engine's `World::field_light_draws` scales the extents by `H / z` at
//! a **world-unit** eye depth, so at a comparable framing its ellipse is six
//! times retail's (divided by whatever the follow camera's distance preset
//! adds). This test does not assert the engine's projection; it pins the
//! retail numbers the fix has to reproduce.
//!
//! Disc- and capture-gated: skip-passes without `LEGAIA_DISC_BIN`,
//! `extracted/`, or the save library directory.

use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::world::ScriptActorRef;
use std::path::PathBuf;

/// `drake_castle_to_worldmap` (retail SCUS; `patch_taint_audit.py states`).
const DOLK_STATE: &str = "0f659b1f3186f549a2cbac7617b4e5597c5bf4ba91e4f9c8fefe38c4e65b58a5";
/// The attached light's per-frame tick, in the actor's `+0x0C` callback
/// word (the word `FUN_8003CF04` keys the op's duplicate check on).
const LIGHT_TICK: u32 = 0x801E_4470;
/// Field actor list heads walked by the frame passes.
const LIST_HEADS: [u32; 6] = [
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
];
/// The active player actor pointer.
const PLAYER_PTR: u32 = 0x8007_C364;
/// The field view matrix `FUN_800172C0` composes in the scratchpad.
const VIEW_MATRIX: u32 = 0x1F80_03C8;

fn find_dir(rel: &str, marker: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{rel}")))
        .find(|d| d.join(marker).exists())
}

struct RetailLight {
    ext: (i16, i16),
    color_a: u32,
    color_b: u32,
    abr: u8,
    offset: (i16, i16, i16),
    script: u32,
    parent: u32,
}

fn retail_light(st: &legaia_pcsxr::SaveState) -> Option<RetailLight> {
    let in_ram = |a: u32| (0x8000_0000..0x8020_0000).contains(&a);
    for head in LIST_HEADS {
        let mut a = st.u32_at(head);
        let mut n = 0;
        while a != 0 && in_ram(a) && n < 400 {
            if st.u32_at(a + 0x0C) == LIGHT_TICK {
                return Some(RetailLight {
                    ext: (st.i16_at(a + 0x3C), st.i16_at(a + 0x3E)),
                    color_a: st.u32_at(a + 0x74),
                    color_b: st.u32_at(a + 0x88),
                    abr: st.u8_at(a + 0x5A),
                    offset: (
                        st.i16_at(a + 0x14),
                        st.i16_at(a + 0x16),
                        st.i16_at(a + 0x18),
                    ),
                    script: st.u32_at(a + 0x94),
                    parent: st.u32_at(a + 0x90),
                });
            }
            a = st.u32_at(a);
            n += 1;
        }
    }
    None
}

/// The light pool's outer rim, from the frame's packets: the flat
/// semi-transparent quads (`0x2A` / `0x2B`) in colour B run from the rim to
/// the screen edge, and their first two vertices sit on the rim, so the
/// bounding box of those vertices is the ellipse's. Returns
/// `(centre_x, centre_y, half_w, half_h)`.
fn packet_rim(st: &legaia_pcsxr::SaveState, rgb: [u8; 3]) -> Option<(i32, i32, i32, i32)> {
    let ram = st.main_ram();
    let (mut x0, mut x1, mut y0, mut y1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    let mut i = 0;
    while i + 24 <= ram.len() {
        if ram[i..i + 3] == rgb && (ram[i + 3] & 0xFE) == 0x2A {
            for k in 0..2 {
                let x = i16::from_le_bytes([ram[i + 4 + 4 * k], ram[i + 5 + 4 * k]]) as i32;
                let y = i16::from_le_bytes([ram[i + 6 + 4 * k], ram[i + 7 + 4 * k]]) as i32;
                (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
            }
        }
        i += 4;
    }
    (x0 <= x1).then(|| ((x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2, (y1 - y0) / 2))
}

#[test]
fn dolk_darkness_mask_matches_the_retail_capture() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = find_dir("extracted", "PROT.DAT") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let Some(lib) = find_dir("saves/library/pcsx-redux", ".") else {
        eprintln!("[skip] saves/library/pcsx-redux missing (capture-gated)");
        return;
    };
    let path = lib.join(format!("{DOLK_STATE}.sstate"));
    if !path.exists() {
        eprintln!("[skip] {} not in the library", path.display());
        return;
    }
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: single-threaded test setup before any save load.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    let st = legaia_pcsxr::SaveState::from_path(&path).expect("load the dolk state");
    assert_eq!(st.scene_name(), "dolk", "the capture is the dolk field");

    // 1. The record, retail vs engine.
    let r = retail_light(&st).expect("an attached light in the retail actor lists");
    assert_eq!(
        r.parent,
        st.u32_at(PLAYER_PTR),
        "retail's light rides the player"
    );
    assert_eq!(r.script, 0, "no keyframe script on the dolk mask");

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("dolk", 0).expect("enter dolk");
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let l = host
        .world
        .script_actors
        .lights
        .iter()
        .find(|l| l.parent == ScriptActorRef::Player)
        .expect("the engine seats the same light on the player");
    assert_eq!(l.sprite.half_extent, r.ext, "extents");
    assert_eq!(l.sprite.color_a, r.color_a, "centre colour");
    assert_eq!(l.sprite.color_b, r.color_b, "rim colour");
    assert_eq!(l.sprite.abr, r.abr, "blend mode");
    assert_eq!(l.sprite.offset, r.offset, "offset from the parent");
    eprintln!(
        "[ok] record: ext {:?} A {:06X} B {:06X} abr {} offset {:?}",
        r.ext, r.color_a, r.color_b, r.abr, r.offset
    );

    // 2. The retail projection: view = M * p + T through the scratchpad
    // view matrix (MVMVA, sf = 1), extents added in that space.
    let Some(sp) = st.scratchpad() else {
        eprintln!("[skip] state carries no scratchpad - record half only");
        return;
    };
    let base = (VIEW_MATRIX - 0x1F80_0000) as usize;
    let h16 = |o: usize| i16::from_le_bytes([sp[base + o], sp[base + o + 1]]) as i64;
    let w32 = |o: usize| {
        i32::from_le_bytes([
            sp[base + o],
            sp[base + o + 1],
            sp[base + o + 2],
            sp[base + o + 3],
        ]) as i64
    };
    let m: Vec<i64> = (0..9).map(|k| h16(2 * k)).collect();
    let t = [w32(0x14), w32(0x18), w32(0x1C)];
    let pl = st.u32_at(PLAYER_PTR);
    let p = [
        st.i16_at(pl + 0x14) as i64 + r.offset.0 as i64,
        st.i16_at(pl + 0x16) as i64 + r.offset.1 as i64,
        st.i16_at(pl + 0x18) as i64 + r.offset.2 as i64,
    ];
    let view: Vec<i64> = (0..3)
        .map(|i| ((m[3 * i] * p[0] + m[3 * i + 1] * p[1] + m[3 * i + 2] * p[2]) >> 12) + t[i])
        .collect();
    let scale = st.i16_at(0x8007_BF10) as i64;
    assert_eq!(
        scale, 24576,
        "the field base matrix is six times world scale"
    );
    let rgb = [
        (r.color_b >> 16) as u8,
        (r.color_b >> 8) as u8,
        r.color_b as u8,
    ];
    // GTE H is the live `_DAT_8007B6F4` (the camera ease's sixth channel).
    let hh = st.i16_at(0x8007_B6F4) as i64;
    let (cx, cy, rx, ry) = packet_rim(&st, rgb).expect("rim packets in the frame");
    let predicted = hh * r.ext.0 as i64 / view[2];
    let px = 160 + hh * view[0] / view[2];
    eprintln!(
        "[ok] retail: H {hh}, view {view:?}, rim centre ({cx},{cy}) half ({rx},{ry}) px; \
         H*ext/vz = {predicted} px, centre x predicted {px}; the extent taken as world \
         units (x6 through the base matrix) would give {} px",
        predicted * scale / 4096
    );
    assert!(
        (rx as i64 - predicted).abs() <= 2 && (ry as i64 - predicted).abs() <= 2,
        "retail's rim is H*ext/vz in the scaled view space: rim ({rx},{ry}) vs {predicted}"
    );
    assert!(
        (cx as i64 - px).abs() <= 2,
        "rim centre x {cx} vs projected {px}"
    );
}
