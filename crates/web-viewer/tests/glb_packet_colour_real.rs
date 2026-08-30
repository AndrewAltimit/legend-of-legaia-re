//! Disc-gated: an exported `.glb` must carry the same shading the canvas
//! draws - asserted on the **exported bytes**, not on the call that wrote
//! them.
//!
//! Retail's whole lighting model on a textured prim is the PSX GPU's texture
//! blend, `texel * colour / 128`, where `colour` is the TMD prim's baked
//! packet word. The browser canvas has applied it for a while (the
//! `a_flat_rgba` attribute); the `.glb` exporters did not, so a model whose
//! colour lives entirely in the packet word exported as the bare texture. The
//! visible case: a summon's sword blade is a near-white texture ramp tinted
//! per vertex - packet words as strong as `(248, 128, 0)` over a `(222, 222,
//! 222)` texel - which renders as a red/orange flame gradient on the page and
//! exported as a flat white blade.
//!
//! Three checks, each answering a different way the export could still be
//! wrong:
//!
//! 1. **Presence with content** - every textured primitive carries a
//!    `COLOR_0` accessor, and across the model the values are not all 1.0
//!    (a stream of neutral words would pass a presence-only assertion while
//!    losing everything).
//! 2. **Value** - each exported vertex colour, re-encoded through the sRGB
//!    OETF, equals its mesh packet word divided by 128, vertex for vertex,
//!    against the same stream the canvas uploads (the file stores the
//!    linearized ratio so a linear-space glTF viewer lands on the canvas
//!    product - see `legaia_asset::gltf_color`).
//! 3. **Non-vacuity** - the cast under test really does carry strongly
//!    non-neutral words, so the value check has something to catch.
//!
//! No Sony bytes are asserted - only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_asset::gltf_color::glb_probe;
use legaia_web_viewer::summon_view::LegaiaSummons;

/// Meta, the armoured summon whose swords carry the flame gradient.
const SWORD_SUMMON: u32 = 0x9E;

fn loaded() -> Option<LegaiaSummons> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut s = LegaiaSummons::new();
    s.load_disc(bytes).ok()?;
    Some(s)
}

/// Every `COLOR_0` value in the file, keyed by nothing - the export's
/// per-object partition reorders vertices, so the multiset is what compares.
fn exported_color0(glb: &[u8]) -> Vec<[f32; 4]> {
    let (root, bin) = glb_probe::split(glb).expect("exported bytes are a .glb");
    let mut out = Vec::new();
    for mesh in root["meshes"].as_array().expect("meshes") {
        for prim in mesh["primitives"].as_array().expect("primitives") {
            let acc = prim["attributes"]["COLOR_0"]
                .as_u64()
                .unwrap_or_else(|| panic!("textured primitive without COLOR_0: {prim}"))
                as usize;
            for row in glb_probe::floats(&root, bin, acc).expect("COLOR_0 is a float accessor") {
                out.push([row[0], row[1], row[2], row[3]]);
            }
        }
    }
    out
}

/// Round the way a `f32` from the file compares against `word / 128`: the
/// file carries the sRGB-linearized ratio, so re-encode it first.
fn to_word(v: f32) -> u32 {
    (legaia_asset::gltf_color::linear_to_srgb_ratio(v) * 128.0).round() as u32
}

#[test]
fn exported_summon_glb_carries_the_canvas_packet_colours() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let row: serde_json::Value =
        serde_json::from_str(&s.set_cast(SWORD_SUMMON)).expect("set_cast is JSON");
    assert_eq!(row["ok"], true, "cast {SWORD_SUMMON:#04x}: {row}");

    // The stream the canvas uploads: [r, g, b, 255] per vertex.
    let canvas = s.mesh_flat_rgba();
    assert!(!canvas.is_empty(), "the page has a packet-colour stream");

    // Non-vacuity: this cast's words really are strongly non-neutral, so a
    // white export would be a visible loss rather than a rounding one.
    let words: Vec<[u8; 3]> = canvas.chunks_exact(4).map(|c| [c[0], c[1], c[2]]).collect();
    let non_neutral = words.iter().filter(|w| **w != [0x80; 3]).count();
    let hot = words.iter().filter(|w| w[0] > 0xC0 && w[2] < 0x40).count();
    assert!(
        non_neutral * 2 > words.len(),
        "most of this cast's words are non-neutral ({non_neutral}/{})",
        words.len()
    );
    assert!(hot > 0, "the flame-tinted blade words are present");

    let glb = s.export_summon_glb();
    assert!(!glb.is_empty(), "the cast exports a .glb");
    let exported = exported_color0(&glb);
    assert_eq!(
        exported.len(),
        words.len(),
        "one COLOR_0 entry per canvas vertex"
    );

    // (1) content, not presence: a white stream would pass an is_number check.
    let white = exported
        .iter()
        .filter(|c| c[0] == 1.0 && c[1] == 1.0 && c[2] == 1.0)
        .count();
    assert!(
        white * 2 < exported.len(),
        "{white}/{} exported colours are neutral white - the packet stream was dropped",
        exported.len()
    );
    // The over-bright tail survives the float encoding (a normalized-ubyte
    // COLOR_0 or a clamp would flatten it to 1.0).
    assert!(
        exported
            .iter()
            .any(|c| c[0] > 1.0 || c[1] > 1.0 || c[2] > 1.0),
        "words above 0x80 export above 1.0"
    );

    // (2) value: the exported multiset re-encodes to the canvas words / 128.
    let mut want: Vec<[u32; 3]> = words
        .iter()
        .map(|w| [w[0].into(), w[1].into(), w[2].into()])
        .collect();
    let mut got: Vec<[u32; 3]> = exported
        .iter()
        .map(|c| {
            assert_eq!(c[3], 1.0, "COLOR_0 alpha is opaque");
            [to_word(c[0]), to_word(c[1]), to_word(c[2])]
        })
        .collect();
    want.sort_unstable();
    got.sort_unstable();
    assert_eq!(
        got, want,
        "every exported colour re-encodes to its packet word / 128"
    );
}

/// The same law across the whole cast list: no export may come back white
/// when its canvas stream is not.
#[test]
fn no_cast_exports_a_whiter_model_than_it_draws() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cat: serde_json::Value = serde_json::from_str(&s.catalog()).expect("catalog is JSON");
    let casts = cat["casts"].as_array().expect("casts").clone();
    assert_eq!(casts.len(), 32, "every player cast is in the catalog");
    let mut checked = 0usize;
    for c in &casts {
        let id = c["spell_id"].as_u64().unwrap() as u32;
        if serde_json::from_str::<serde_json::Value>(&s.set_cast(id)).unwrap()["ok"] != true {
            continue;
        }
        let canvas = s.mesh_flat_rgba();
        let canvas_non_neutral = canvas
            .chunks_exact(4)
            .filter(|c| [c[0], c[1], c[2]] != [0x80; 3])
            .count();
        let glb = s.export_summon_glb();
        assert!(!glb.is_empty(), "cast {id:#04x} exports a .glb");
        let exported = exported_color0(&glb);
        let exported_non_neutral = exported.iter().filter(|c| c[0..3] != [1.0; 3]).count();
        assert_eq!(
            exported_non_neutral, canvas_non_neutral,
            "cast {id:#04x}: the export keeps every shaded vertex the canvas shades"
        );
        checked += 1;
    }
    assert_eq!(checked, 32, "every cast was exercised");
}
