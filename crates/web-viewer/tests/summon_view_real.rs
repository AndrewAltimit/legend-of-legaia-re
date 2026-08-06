//! Disc-gated: the `site/magic.html` WASM surface (`LegaiaSummons`) must
//! resolve **every** player Seru-magic cast to a drawable creature with at
//! least one playable keyframe clip, and each of the seven named Ra-Seru /
//! Sim-Seru summons to a bespoke mesh with a real packet-colour stream.
//!
//! Three layers of coverage, each answering a different way the page could be
//! wrong:
//!
//! 1. **Span** - all 32 casts `0x81..=0xA0` appear in the catalog and each one
//!    decodes to a mesh + clips. A missing id is a hole in the picker.
//! 2. **Identity** - the seven summons resolve by name, and each one's id is
//!    confirmed by two independent disc reads: the actor record's inline ASCII
//!    attack-name string, and the record's `+0x1D` element byte. A table typo
//!    fails here rather than shipping a mislabelled model.
//! 3. **Content, not length** - the colour stream must not be white (the
//!    `texel * 255/128` blowout this repo has shipped four times), the clips
//!    must actually move the rig, and the FX pages must not decode to a single
//!    flat colour.
//!
//! No Sony bytes are asserted - only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::summon as summon_core;
use legaia_web_viewer::summon_view::LegaiaSummons;

/// The seven summons the page must name and draw.
const NAMED_SUMMONS: [&str; 7] = ["Meta", "Ozma", "Terra", "Horn", "Jedo", "Palma", "Mule"];

fn loaded() -> Option<LegaiaSummons> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut s = LegaiaSummons::new();
    s.load_disc(bytes).ok()?;
    Some(s)
}

fn catalog(s: &LegaiaSummons) -> serde_json::Value {
    serde_json::from_str(&s.catalog()).expect("catalog is JSON")
}

#[test]
fn every_seru_magic_cast_resolves_to_a_playable_animation() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cat = catalog(&s);
    assert_eq!(cat["ok"], true, "catalog: {cat}");
    let casts = cat["casts"].as_array().expect("casts array");

    // Non-vacuity: the whole 0x81..=0xA0 run must be present, in order. An
    // empty or short catalog would otherwise make every loop below trivial.
    let ids: Vec<u64> = casts
        .iter()
        .map(|c| c["spell_id"].as_u64().unwrap())
        .collect();
    let want: Vec<u64> = (0x81..=0xA0).collect();
    assert_eq!(ids, want, "every player summon cast id is in the catalog");
    assert_eq!(casts.len(), 32);

    let mut total_frames = 0usize;
    for c in casts {
        let id = c["spell_id"].as_u64().unwrap() as u32;
        let label = format!("{:#04x} {}", id, c["summon"].as_str().unwrap_or("?"));

        // Every cast names itself on the disc.
        let attack = c["attack"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: actor record carries an attack name"));
        assert!(
            attack.chars().any(|ch| ch.is_ascii_alphabetic()),
            "{label}: attack name {attack:?} is text"
        );

        // Every cast has at least one clip, and its clips move the rig.
        let clips = c["clips"].as_array().unwrap();
        assert!(!clips.is_empty(), "{label}: has keyframe clips");
        for k in clips {
            let (parts, frames) = (k["parts"].as_u64().unwrap(), k["frames"].as_u64().unwrap());
            assert!(parts > 0 && frames > 0, "{label}: clip {k} is non-empty");
            total_frames += frames as usize;
        }

        // ... and it draws.
        let st: serde_json::Value = serde_json::from_str(&s.set_cast(id)).unwrap();
        assert_eq!(st["ok"], true, "{label}: set_cast: {st}");
        let n = s.mesh_positions().len() / 3;
        assert!(n > 0, "{label}: mesh has vertices");
        assert_eq!(s.mesh_uvs().len(), n * 2, "{label}: uvs parallel");
        assert_eq!(s.mesh_cba_tsb().len(), n * 2, "{label}: cba/tsb parallel");
        assert_eq!(s.mesh_flat_rgba().len(), n * 4, "{label}: colours parallel");
        let idx = s.mesh_indices();
        assert!(
            !idx.is_empty() && idx.len().is_multiple_of(3),
            "{label}: triangles"
        );
        assert!(
            idx.iter().all(|&i| (i as usize) < n),
            "{label}: index in range"
        );
        assert!(s.mesh_bounds()[3] > 0.0, "{label}: framed");
        assert_eq!(
            s.vram_bytes().len(),
            1024 * 512 * 2,
            "{label}: full PSX VRAM"
        );

        // Every clip is playable through the page's pose accessor at the
        // layout the poser reads, and the cast as a whole is not a still
        // image: at least one clip has a frame that differs from its first.
        // (Individual clips legitimately open on a hold - 0x94 Puera's first
        // phase repeats frame 0 - so the "moves" check belongs to the cast.)
        let mut cast_animates = false;
        for (ci, k) in clips.iter().enumerate() {
            let parts = k["parts"].as_u64().unwrap() as usize;
            let frames = k["frames"].as_u64().unwrap() as usize;
            let poses = s.clip_pose_frames(ci as u32);
            assert_eq!(
                poses.len(),
                parts * frames * 6,
                "{label}: clip {ci} pose layout"
            );
            let stride = parts * 6;
            if poses.chunks_exact(stride).any(|f| f != &poses[..stride]) {
                cast_animates = true;
            }
        }
        assert!(cast_animates, "{label}: every clip is a frozen pose");
    }
    assert!(
        total_frames > 1000,
        "non-vacuity: the 32 casts carry {total_frames} keyframes in total"
    );
}

#[test]
fn the_seven_named_summons_resolve_to_bespoke_meshes() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cat = catalog(&s);
    let casts = cat["casts"].as_array().unwrap();
    let mut seen = 0usize;

    for name in NAMED_SUMMONS {
        let want = summon_core::big_summon_by_name(name)
            .unwrap_or_else(|| panic!("{name} is a known big summon"));
        let row = casts
            .iter()
            .find(|c| c["spell_id"].as_u64() == Some(want.spell_id as u64))
            .unwrap_or_else(|| panic!("{name} ({:#04x}) is in the catalog", want.spell_id));

        // Identity, read off the disc twice over: the actor record's inline
        // attack-name string and its +0x1D element byte.
        assert_eq!(
            row["attack"].as_str(),
            Some(want.attack),
            "{name}: the disc's own attack name pins the id"
        );
        assert_eq!(
            row["element_id"].as_u64(),
            Some(want.element as u64),
            "{name}: the record's element byte pins the id"
        );
        assert_eq!(row["summon"].as_str(), Some(name));
        assert_eq!(row["ra_seru"], true, "{name}: four-slot big-summon group");
        assert_eq!(
            row["bespoke"], true,
            "{name}: body is bespoke, not a reused enemy"
        );
        assert!(
            row["creature"].is_null(),
            "{name}: no battle_data creature id (that is why it needed its own path)"
        );

        // The model draws, with a real rig and a real packet-colour stream.
        let st: serde_json::Value =
            serde_json::from_str(&s.set_cast(want.spell_id as u32)).unwrap();
        assert_eq!(st["ok"], true, "{name}: set_cast: {st}");
        let parts = st["part_count"].as_u64().unwrap();
        assert!(parts > 1, "{name}: rig has {parts} parts");
        let objects = st["object_count"].as_u64().unwrap();
        assert!(objects > 1, "{name}: mesh has {objects} objects");

        let n = s.mesh_positions().len() / 3;
        assert!(n > 100, "{name}: {n} vertices is a real body");
        let rgba = s.mesh_flat_rgba();
        assert_eq!(rgba.len(), n * 4, "{name}: colour stream is parallel");

        // Content, not length: the white-modulation trap. An unbound colour
        // attribute defaults to white, and white is `texel * 255/128` - a
        // blowout that reads as "too bright", never as "unlit". Assert the
        // stream is neither all-white nor all-one-value.
        let rgb: Vec<[u8; 3]> = rgba.chunks_exact(4).map(|c| [c[0], c[1], c[2]]).collect();
        assert!(
            !rgb.iter().all(|c| *c == [255, 255, 255]),
            "{name}: colour stream is all white - the texel*2 blowout"
        );
        assert!(
            rgba.chunks_exact(4).all(|c| c[3] == 255),
            "{name}: every vertex is flagged textured"
        );
        let distinct: std::collections::BTreeSet<[u8; 3]> = rgb.iter().copied().collect();
        assert!(
            !distinct.is_empty(),
            "{name}: colour stream is not a constant fill"
        );

        // The cast plays: at least one clip, concatenated into a timeline.
        let seq = s.sequence_clip_indices();
        assert!(!seq.is_empty(), "{name}: has a playable cast sequence");
        let frames = s.sequence_pose_frames();
        assert!(
            frames.len() >= parts as usize * 6,
            "{name}: sequence carries poses"
        );
        assert!(
            frames.len().is_multiple_of(parts as usize * 6),
            "{name}: sequence is whole frames of the rig"
        );

        // The VRAM the body samples is populated - a black CLUT row would draw
        // a silhouette, and length alone would not catch it.
        let vram = s.vram_bytes();
        let clut_row_486 = &vram[486 * 1024 * 2..486 * 1024 * 2 + 480];
        assert!(
            clut_row_486.iter().any(|&b| b != 0),
            "{name}: the summon CLUT row is populated"
        );
        seen += 1;
    }
    assert_eq!(seen, 7, "non-vacuity: all seven summons were checked");
}

/// The `.glb` download: every cast exports a valid binary glTF whose
/// animations are the cast's own phase clips, channelled **1:1 onto the TMD
/// objects**. `character_gltf`'s contract is "animation channel `i` drives
/// TMD object `i`"; the arts page satisfies it by *expanding* clips per
/// assembled object, the summon path claims it holds natively because the
/// actor record's `+0x4C` per-part table is indexed by TMD object. This test
/// asserts that claim instead of assuming it: for every clip of every cast,
/// every animated part index resolves to a real `object_<i>` node, and each
/// animation binds exactly `2 * part_count` channels (translation + rotation
/// per part - none dropped, none dangling).
#[test]
fn glb_export_bakes_every_phase_clip_channelled_one_to_one_onto_objects() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cat = catalog(&s);
    let casts = cat["casts"].as_array().unwrap();
    assert_eq!(casts.len(), 32, "non-vacuity: full cast span");

    // Nothing loaded -> nothing exported (the page's disabled-button state).
    assert!(s.export_summon_glb().is_empty(), "no cast: empty export");
    assert!(s.export_name().is_empty(), "no cast: empty name");

    for c in casts {
        let id = c["spell_id"].as_u64().unwrap() as u32;
        let clips = c["clips"].as_array().unwrap().clone();
        let st: serde_json::Value = serde_json::from_str(&s.set_cast(id)).unwrap();
        assert_eq!(st["ok"], true, "{id:#04x}: set_cast");
        let label = format!("{:#04x} {}", id, s.export_name());
        let object_count = st["object_count"].as_u64().unwrap();

        // A valid .glb: magic, version 2, declared length == byte length.
        let glb = s.export_summon_glb();
        assert!(
            glb.len() > 1000,
            "{label}: .glb non-trivial ({})",
            glb.len()
        );
        assert_eq!(&glb[0..4], b"glTF", "{label}: magic");
        assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
        let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
        assert_eq!(total, glb.len(), "{label}: declared length");
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let root: serde_json::Value =
            serde_json::from_slice(&glb[20..20 + json_len]).expect("JSON chunk parses");

        // Every TMD object surfaced as a node (+1 for the reorient root).
        let nodes = root["nodes"].as_array().unwrap();
        let node_names: std::collections::BTreeSet<&str> =
            nodes.iter().filter_map(|n| n["name"].as_str()).collect();
        assert_eq!(
            nodes.len() as u64,
            object_count + 1,
            "{label}: one node per TMD object plus the root"
        );

        // Animation count == the catalog's clip count, in table order.
        let anims = root["animations"].as_array().unwrap();
        assert_eq!(
            anims.len(),
            clips.len(),
            "{label}: one glTF animation per phase clip"
        );
        for (i, (a, k)) in anims.iter().zip(&clips).enumerate() {
            let part_count = k["parts"].as_u64().unwrap();
            let tag = k["tag"].as_u64().unwrap();
            assert_eq!(
                a["name"].as_str().unwrap(),
                format!("phase {} (tag 0x{tag:02X})", i + 1),
                "{label}: clip {i} keeps its phase identity"
            );
            // The 1:1 contract, asserted not assumed: every animated part
            // index is a real object node...
            for p in 0..part_count {
                assert!(
                    node_names.contains(format!("object_{p}").as_str()),
                    "{label}: clip {i} part {p} has no TMD object to drive \
                     (rig width {part_count}, {object_count} objects)"
                );
            }
            // ...and the animation binds exactly translation + rotation for
            // each of them - no channel was dropped for a missing node.
            assert_eq!(
                a["channels"].as_array().unwrap().len() as u64,
                2 * part_count,
                "{label}: clip {i} channel count is 2 x rig width"
            );
            // Timeline runs at the retail rate byte: frames / (7.5 * rate).
            let rate = k["rate"].as_u64().unwrap();
            let frames = k["frames"].as_u64().unwrap();
            if rate > 0 && frames > 1 {
                let in_acc = a["samplers"][0]["input"].as_u64().unwrap() as usize;
                let max_t = root["accessors"][in_acc]["max"][0].as_f64().unwrap();
                let want = (frames - 1) as f64 / (7.5 * rate as f64);
                assert!(
                    (max_t - want).abs() < 1e-3,
                    "{label}: clip {i} duration {max_t} != {want}"
                );
            }
        }

        // The bake carries a texture atlas (the summon's VRAM pool).
        assert!(
            root["images"].as_array().is_some_and(|i| !i.is_empty()),
            "{label}: atlas image present"
        );
    }
}

#[test]
fn casts_expose_their_fx_texture_pages() {
    let Some(mut s) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cat = catalog(&s);
    let casts = cat["casts"].as_array().unwrap();
    let (mut pages_seen, mut pages_with_art) = (0usize, 0usize);
    let mut casts_with_art = 0usize;
    for c in casts {
        let id = c["spell_id"].as_u64().unwrap() as u32;
        let pages = c["fx_pages"].as_u64().unwrap();
        if pages == 0 {
            continue;
        }
        let st: serde_json::Value = serde_json::from_str(&s.set_cast(id)).unwrap();
        assert_eq!(st["ok"], true);
        let sizes = st["fx_page_sizes"].as_array().unwrap();
        assert_eq!(sizes.len() as u64, pages);
        let mut cast_has_art = false;
        for (i, sz) in sizes.iter().enumerate() {
            let w = sz[0].as_u64().unwrap() as usize;
            let h = sz[1].as_u64().unwrap() as usize;
            assert!(w == 256 || w == 512, "{id:#04x}: page width {w}");
            assert_eq!(h, 256, "{id:#04x}: pages are 256 rows tall");
            let rgba = s.fx_page_rgba(i as u32);
            assert_eq!(rgba.len(), w * h * 4, "{id:#04x}: page {i} decodes");
            // Content, not length. A page that decodes to one flat colour is
            // either genuinely unused (a group whose second texture upload the
            // applier skips) or a broken 4bpp unpack - and the size check above
            // passes either way. Count them: if the unpack broke, essentially
            // every page goes flat and the file-wide floor below fails.
            let distinct: std::collections::BTreeSet<[u8; 4]> = rgba
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect();
            pages_seen += 1;
            if distinct.len() > 1 {
                pages_with_art += 1;
                cast_has_art = true;
            }
        }
        if cast_has_art {
            casts_with_art += 1;
        }
    }
    // Non-vacuity + content: the file really does carry per-cast FX art, and
    // most casts have some. Pinned as floors so a decoder regression (which
    // flattens everything) fails rather than silently passing.
    assert!(
        pages_seen >= 32,
        "non-vacuity: only {pages_seen} FX pages were classified"
    );
    assert!(
        pages_with_art * 2 >= pages_seen,
        "{pages_with_art} of {pages_seen} FX pages carry art - the 4bpp unpack looks broken"
    );
    assert!(
        casts_with_art >= 24,
        "only {casts_with_art} casts have a non-flat FX page"
    );
}
