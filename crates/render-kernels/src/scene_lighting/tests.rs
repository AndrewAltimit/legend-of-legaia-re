use super::*;

fn flame_quad(center: [f32; 3], half: f32) -> ([[f32; 3]; 4], [u32; 6]) {
    let [x, y, z] = center;
    (
        [
            [x - half, y - half, z],
            [x + half, y - half, z],
            [x - half, y + half, z],
            [x + half, y + half, z],
        ],
        [0, 1, 2, 1, 3, 2],
    )
}

const ADDITIVE: u16 = SEMI_BIT | (1 << 5);

/// An additive (ABR 1) semi prim is an emitter even at neutral
/// modulation colour; an opaque prim never is.
#[test]
fn additive_prims_are_emitters() {
    let (pos, idx) = flame_quad([100.0, -80.0, 40.0], 16.0);
    let cba_tsb = [[0u16, ADDITIVE]; 4];
    let colors = [[0x80u8; 3]; 4];
    let got = vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx, None);
    assert_eq!(got.len(), 2, "both triangles of the flame quad");
    let plain = [[0u16, 0u16]; 4];
    assert!(vram_mesh_emitters(&pos, &plain, &colors, &idx, None).is_empty());
}

/// A bright-warm ABE prim (mode 0) counts; a blue one (water) doesn't.
#[test]
fn bright_warm_gate() {
    let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
    let cba_tsb = [[0u16, SEMI_BIT]; 4];
    let warm = [[0xE0u8, 0xB0, 0x60]; 4];
    assert_eq!(
        vram_mesh_emitters(&pos, &cba_tsb, &warm, &idx, None).len(),
        2
    );
    let blue = [[0x40u8, 0x80, 0xE0]; 4];
    assert!(vram_mesh_emitters(&pos, &cba_tsb, &blue, &idx, None).is_empty());
}

/// Big additive sheets (water) are rejected by the area cap.
#[test]
fn big_sheets_are_rejected() {
    let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 500.0);
    let cba_tsb = [[0u16, ADDITIVE]; 4];
    let colors = [[0xFFu8, 0xC0, 0x60]; 4];
    assert!(vram_mesh_emitters(&pos, &cba_tsb, &colors, &idx, None).is_empty());
}

/// Colour-mesh emitters key off the packed blend word + warm colour.
#[test]
fn color_mesh_emitters_gate_on_abe() {
    let (pos, idx) = flame_quad([0.0, 0.0, 0.0], 16.0);
    let warm = [[0xF0u8, 0xC0, 0x60]; 4];
    let on = [SEMI_BIT; 4];
    let off = [0u16; 4];
    assert_eq!(color_mesh_emitters(&pos, &warm, &on, &idx).len(), 2);
    assert!(color_mesh_emitters(&pos, &warm, &off, &idx).is_empty());
}

/// The emissive tag sets bit 13 on exactly the glowing prims' corners
/// and leaves every other bit of the word alone.
#[test]
fn tagging_sets_only_the_emissive_bit() {
    let (_, idx) = flame_quad([0.0; 3], 8.0);
    let pos = [[0.0f32; 3]; 4];
    let uvs = [[0u8; 2]; 4];
    let mut ct = [[0x1234u16, ADDITIVE | 0x0105]; 4];
    let colors = [[0x80u8; 3]; 4];
    assert_eq!(
        tag_emissive_vram(&pos, &mut ct, &colors, &uvs, &idx, None).0,
        2
    );
    for c in ct {
        assert_eq!(c[0], 0x1234, "CBA untouched");
        assert_eq!(c[1], ADDITIVE | 0x0105 | EMISSIVE_BIT);
    }
    let mut plain = [[0u16, 0x0105u16]; 4];
    assert_eq!(
        tag_emissive_vram(&pos, &mut plain, &colors, &uvs, &idx, None).0,
        0
    );
    assert!(plain.iter().all(|c| c[1] == 0x0105));
    // A curated whole-mesh entry tags everything, and reports where.
    let e = EmissiveMesh {
        signature: 0,
        label: "test",
        texels: CuratedTexels::All,
        light: [1.0; 3],
        radius: 800.0,
    };
    let vram = legaia_tim::Vram::new();
    let (n, center) = tag_emissive_vram(&pos, &mut plain, &colors, &uvs, &idx, Some((&e, &vram)));
    assert_eq!(n, 2);
    assert_eq!(center, Some([0.0; 3]));
    assert!(plain.iter().all(|c| c[1] & EMISSIVE_BIT != 0));
}

/// The emissive bit collides with no bit a retail TSB, a blend word or
/// the engine's own packings use.
#[test]
fn emissive_bit_is_free() {
    const TSB_USED: u16 = 0x01FF; // page x/y, ABR, depth
    const DOUBLE_SIDED_BLEND: u16 = 0x4000;
    assert_eq!(EMISSIVE_BIT & (TSB_USED | SEMI_BIT | DOUBLE_SIDED_BLEND), 0);
}

/// Hybrid tagging reads an untextured vertex's fill colour from the
/// flat side channel, not its neutral modulation word.
#[test]
fn hybrid_tagging_reads_the_fill() {
    let (pos, idx) = flame_quad([0.0; 3], 8.0);
    let mesh = |cba_tsb: [u16; 2]| legaia_tmd::mesh::VramMesh {
        positions: pos.to_vec(),
        uvs: vec![[0, 0]; 4],
        cba_tsb: vec![cba_tsb; 4],
        indices: idx.to_vec(),
        normals: vec![[0.0; 3]; 4],
        colors: vec![[0x80; 3]; 4],
    };
    let vram = legaia_tim::Vram::new();
    let mut warm_mesh = mesh([0, SEMI_BIT]);
    let flat: Vec<u8> = (0..4).flat_map(|_| [0xF0, 0xC0, 0x60, 0]).collect();
    assert!(tag_emissive_hybrid(&[], &mut warm_mesh, &flat, &vram).is_none());
    assert!(warm_mesh.cba_tsb.iter().all(|c| c[1] & EMISSIVE_BIT != 0));
    let mut dark_mesh = mesh([0, SEMI_BIT]);
    let dark: Vec<u8> = (0..4).flat_map(|_| [0x30, 0x30, 0x30, 0]).collect();
    tag_emissive_hybrid(&[], &mut dark_mesh, &dark, &vram);
    assert!(dark_mesh.cba_tsb.iter().all(|c| c[1] & EMISSIVE_BIT == 0));
}

/// The curated table's keys are unique and every entry has a light.
#[test]
fn curated_table_is_well_formed() {
    for (i, a) in EMISSIVE_MESHES.iter().enumerate() {
        assert!(
            a.radius > 0.0 && a.light.iter().any(|&c| c > 0.0),
            "{}",
            a.label
        );
        for b in &EMISSIVE_MESHES[i + 1..] {
            assert_ne!(a.signature, b.signature);
        }
    }
    assert_eq!(tmd_signature(b""), SIGNATURE_BASIS);
}

/// Nearby samples merge into one light; the cap keeps the strongest.
#[test]
fn clustering_merges_and_caps() {
    let mut samples = Vec::new();
    for dx in [0.0, 20.0] {
        samples.push(EmitterSample {
            pos: [100.0 + dx, -60.0, 100.0],
            color: [1.0, 0.8, 0.5],
            weight: 10.0,
        });
    }
    for i in 0..12 {
        samples.push(EmitterSample {
            pos: [5000.0 + 2000.0 * i as f32, 0.0, 0.0],
            color: [1.0, 1.0, 1.0],
            weight: 1.0,
        });
    }
    let lights = cluster_scene_lights(&samples);
    assert!(lights.len() <= MAX_SCENE_LIGHTS);
    let strongest = &lights[0];
    assert!((strongest.pos[0] - 110.0).abs() < 1.0, "{strongest:?}");
    assert!(strongest.radius >= RADIUS_MIN && strongest.radius <= RADIUS_MAX);
}

/// A cluster that spreads wider than a prop is dropped.
#[test]
fn wide_clusters_are_dropped() {
    let samples: Vec<EmitterSample> = (0..10)
        .map(|i| EmitterSample {
            pos: [i as f32 * 250.0, 0.0, 0.0],
            color: [1.0, 0.9, 0.6],
            weight: 100.0 - i as f32,
        })
        .collect();
    let lights = cluster_scene_lights(&samples);
    for l in &lights {
        assert!(l.radius <= RADIUS_MAX);
    }
}

/// Attenuation: 1 at the light, 0 at the radius, monotonic between.
#[test]
fn attenuation_shape() {
    assert!((point_attenuation(0.0, 1000.0) - 1.0).abs() < 1e-6);
    assert_eq!(point_attenuation(1000.0, 1000.0), 0.0);
    assert_eq!(point_attenuation(1500.0, 1000.0), 0.0);
    let mut last = 1.0;
    for i in 1..10 {
        let a = point_attenuation(i as f32 * 100.0, 1000.0);
        assert!(a < last, "attenuation must decrease");
        last = a;
    }
}

/// The point-gain mirror: straight under the light the wrap term is
/// 1.0, so the gain is colour x attenuation x scale; out of radius zero.
#[test]
fn point_gain_mirror() {
    let l = ScenePointLight {
        pos: [0.0, -100.0, 0.0],
        color: [1.0, 0.8, 0.5],
        radius: 800.0,
    };
    let g = point_gain([0.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l], 2.0);
    let att = point_attenuation(100.0, 800.0);
    assert!((g[0] - 2.0 * att).abs() < 1e-5, "{g:?} vs att {att}");
    assert!((g[1] - 1.6 * att).abs() < 1e-5);
    let far = point_gain([5000.0, 0.0, 0.0], [0.0, -1.0, 0.0], &[l], 1.0);
    assert_eq!(far, [0.0; 3]);
}

/// The load-bearing invariant: enable off is the exact identity, even
/// for an emissive fragment under a huge point gain.
#[test]
fn disabled_is_exact_identity() {
    let rgb = [0.123, 0.456, 0.789];
    let u = LightingMood::NIGHT.uniforms(false);
    for emissive in [false, true] {
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [10.0, 10.0],
            [960.0, 720.0],
            u,
            [9.0; 3],
            emissive,
        );
        assert_eq!(out, rgb);
    }
}

/// Night darkens an unlit surface well below the baked colour while an
/// emissive one stays at (or above) it - the whole point of the mood.
#[test]
fn night_darkens_surfaces_but_not_emissives() {
    let rgb = [0.5; 3];
    let u = LightingMood::NIGHT.uniforms(true);
    let wall = shade(
        rgb,
        [0.0, 0.0, 1.0],
        [0.0, 0.0],
        [960.0, 720.0],
        u,
        [0.0; 3],
        false,
    );
    let lamp = shade(
        rgb,
        [0.0, 0.0, 1.0],
        [0.0, 0.0],
        [960.0, 720.0],
        u,
        [0.0; 3],
        true,
    );
    assert!(wall.iter().all(|&c| c < 0.35), "{wall:?}");
    assert!(lamp.iter().all(|&c| c >= 0.5), "{lamp:?}");
    // A nearby lamp lifts the wall back toward its baked colour.
    let lit = shade(
        rgb,
        [0.0, 0.0, 1.0],
        [0.0, 0.0],
        [960.0, 720.0],
        u,
        [0.6; 3],
        false,
    );
    assert!(lit[0] > wall[0] + 0.2, "{lit:?} vs {wall:?}");
}

/// Daylight stays near the retail brightness on average (no wash-out):
/// an up-facing fragment mid-pool lands within 30% of the baked colour.
#[test]
fn day_is_near_retail_brightness() {
    let rgb = [0.5; 3];
    let u = LightingMood::DAY.uniforms(true);
    let out = shade(
        rgb,
        [0.0, -1.0, 0.0],
        [480.0, 324.0],
        [960.0, 720.0],
        u,
        [0.0; 3],
        false,
    );
    for c in out {
        assert!((0.35..=0.65).contains(&c), "{out:?}");
    }
}

/// The gain never exceeds TOTAL_MAX_GAIN x baked, whatever the lights.
#[test]
fn gain_is_capped() {
    let rgb = [0.4; 3];
    for mood in [
        LightingMood::DAY,
        LightingMood::DUSK,
        LightingMood::NIGHT,
        LightingMood::CAVE,
        LightingMood::SCRIPTED_NIGHT,
        LightingMood::SCRIPTED_DUSK,
    ] {
        let out = shade(
            rgb,
            [0.0, -1.0, 0.0],
            [480.0, 324.0],
            [960.0, 720.0],
            mood.uniforms(true),
            [100.0; 3],
            false,
        );
        for c in out {
            assert!(c <= 0.4 * TOTAL_MAX_GAIN + 1e-6, "{} {out:?}", mood.name);
        }
    }
}

#[test]
fn time_of_day_cycles_and_parses() {
    let mut t = TimeOfDay::Auto;
    let mut seen = vec![t];
    for _ in 0..3 {
        t = t.next();
        seen.push(t);
    }
    assert_eq!(seen, TimeOfDay::ALL.to_vec());
    assert_eq!(t.next(), TimeOfDay::Auto);
    for t in TimeOfDay::ALL {
        assert_eq!(TimeOfDay::from_name(t.name()), Some(t));
    }
    assert_eq!(TimeOfDay::Auto.mood("cave01").name, "cave");
    assert_eq!(TimeOfDay::Auto.mood("town01").name, "day");
    assert_eq!(TimeOfDay::Night.mood("cave01").name, "night");
}

/// A scene the script grades to night is lit as night under `Auto`;
/// a forced time of day still wins, and non-sky grades fall through.
#[test]
fn scripted_grade_picks_the_auto_mood() {
    let night = Some((2, [0x30, 0x30, 0x00]));
    let dusk = Some((2, [0x00, 0x10, 0x18]));
    assert_eq!(TimeOfDay::Auto.mood_graded("town0b", night).name, "night");
    assert_eq!(TimeOfDay::Auto.mood_graded("town01", dusk).name, "dusk");
    assert_eq!(TimeOfDay::Day.mood_graded("town0b", night).name, "day");
    // Additive, grey and black grades are not a time of day.
    for g in [
        (1, [0x30, 0x30, 0x00]),
        (2, [0x40, 0x40, 0x40]),
        (2, [0, 0, 0]),
    ] {
        assert_eq!(TimeOfDay::Auto.mood_graded("town01", Some(g)).name, "day");
    }
    assert_eq!(TimeOfDay::Auto.mood_graded("cave01", None).name, "cave");
}

/// The depth cue's far term darkens at night (the battle grid's
/// near-white far colour must not keep the floor lit) and is untouched
/// while the enhancement is off.
#[test]
fn far_gain_follows_the_mood() {
    assert_eq!(far_gain(LightingMood::NIGHT.uniforms(false)), [1.0; 3]);
    let n = far_gain(LightingMood::NIGHT.uniforms(true));
    assert!(n.iter().all(|&g| g < 0.7) && n[0] < 0.4, "{n:?}");
    let d = far_gain(LightingMood::DAY.uniforms(true));
    assert!(d.iter().all(|&g| (0.75..=MAX_GAIN).contains(&g)), "{d:?}");
}

/// Daylight leaves the ground fog's colour alone; night turns it to a
/// darker moonlit blue.
#[test]
fn fog_tint_follows_the_mood() {
    let c = [0.6, 0.62, 0.7];
    assert_eq!(fog_tint(c, &LightingMood::DAY), c);
    let n = fog_tint(c, &LightingMood::NIGHT);
    assert!(
        n[2] > n[0] && n.iter().zip(c).all(|(a, b)| *a <= b + 1e-6),
        "{n:?}"
    );
}

/// Halo falloff: 1 at the centre, 0 at the rim; shafts fade downward.
#[test]
fn glow_falloff_shape() {
    assert!((glow_falloff([0.0, 0.0], false) - 1.0).abs() < 1e-6);
    assert_eq!(glow_falloff([1.0, 0.0], false), 0.0);
    assert!(glow_falloff([0.0, 0.2], true) > glow_falloff([0.0, 0.8], true));
    assert_eq!(glow_falloff([0.0, 1.0], true), 0.0);
}

/// Two sprites per light, six vertices per sprite; no glow at zero.
#[test]
fn glow_geometry_counts() {
    let l = ScenePointLight {
        pos: [0.0, -200.0, 0.0],
        color: [1.0, 0.8, 0.5],
        radius: 800.0,
    };
    let s = glow_sprites(&[l, l], &LightingMood::NIGHT);
    assert_eq!(s.len(), 4);
    let v = glow_vertices(&s, [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]);
    assert_eq!(v.len(), 24);
    let mut none = LightingMood::NIGHT;
    none.glow = 0.0;
    assert!(glow_sprites(&[l], &none).is_empty());
}

/// The window tag rides a TSB bit no retail word and no other engine
/// tag uses.
#[test]
fn window_bit_is_free() {
    const TSB_USED: u16 = 0x01FF;
    assert_eq!(
        WINDOW_BIT & (TSB_USED | SEMI_BIT | EMISSIVE_BIT | 0x4000),
        0
    );
}

/// Every curated window art is a non-empty rectangle on a real CLUT /
/// page word, and no two entries share a key.
#[test]
fn lit_window_table_is_well_formed() {
    for (i, w) in LIT_WINDOWS.iter().enumerate() {
        assert!(
            w.rect[0] <= w.rect[2] && w.rect[1] <= w.rect[3],
            "{}",
            w.label
        );
        assert_eq!(w.cba & 0x8000, 0, "{}", w.label);
        assert_eq!(w.tpage & !0x1FF, 0, "{}", w.label);
        for o in &LIT_WINDOWS[i + 1..] {
            assert_ne!(
                (w.cba, w.tpage, w.rect, w.hash),
                (o.cba, o.tpage, o.rect, o.hash)
            );
        }
    }
}

/// A prim matches an art only on the same CLUT + page, inside the
/// rectangle, and only while VRAM holds the hashed texels.
#[test]
fn window_art_match_keys_on_place_and_content() {
    let mut vram = legaia_tim::Vram::new();
    // A 4bpp page at x=0 with a CLUT at (0, 480): paint index 1 blue.
    let clut: Vec<u8> = [0u16, 0x7C00, 0x001F]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    vram.write_clut_row(0, 480, &clut);
    vram.write_block(0, 0, 4, 16, &[0x11; 4 * 2 * 16]);
    let cba = 480 << 6;
    let tsb = 0x0000;
    let mut art = WindowArt {
        label: "test",
        cba,
        tpage: 0,
        rect: [0, 0, 15, 15],
        hash: 0,
    };
    art.hash = window_art_hash(&vram, cba, 0, art.rect);
    let inside = [[1, 1], [14, 1], [1, 14]];
    assert!(prim_samples_art(&art, &vram, cba, tsb, &inside));
    assert!(!prim_samples_art(
        &art,
        &vram,
        cba,
        tsb,
        &[[1, 1], [20, 1], [1, 14]]
    ));
    assert!(!prim_samples_art(&art, &vram, cba + 1, tsb, &inside));
    assert!(!prim_samples_art(&art, &vram, cba, tsb | 1, &inside));
    vram.write_block(0, 0, 1, 1, &[0x22, 0x22]);
    assert!(
        !prim_samples_art(&art, &vram, cba, tsb, &inside),
        "content changed"
    );
}

/// The pane law: identity while off, at day, off a window prim or on a
/// non-glass texel; at night a glass texel draws as warm lamp light.
#[test]
fn window_glass_glows_only_at_night_and_only_on_glass() {
    let glass = [0.06, 0.16, 0.42];
    let frame = [0.55, 0.5, 0.48];
    let shutter = [0.45, 0.48, 0.58];
    let lit = [0.1, 0.12, 0.2];
    let night = LightingMood::NIGHT;
    let on = night.uniforms(true);
    let off = night.uniforms(false);
    let g = night.window_word()[0];
    assert_eq!(shade_window(lit, glass, off, g, true), lit);
    assert_eq!(shade_window(lit, glass, on, g, false), lit);
    assert_eq!(shade_window(lit, frame, on, g, true), lit);
    assert_eq!(shade_window(lit, shutter, on, g, true), lit);
    let day = LightingMood::DAY;
    assert_eq!(
        shade_window(lit, glass, day.uniforms(true), day.window_word()[0], true),
        lit
    );
    let void = [0.03, 0.03, 0.06];
    let mullion = [0.19, 0.19, 0.23];
    assert_eq!(shade_window(lit, mullion, on, g, true), lit);
    assert_ne!(shade_window(lit, void, on, g, true), lit);
    let out = shade_window(lit, glass, on, g, true);
    assert!(out[0] > out[2] && out[0] > 0.6, "{out:?}");
    let dusk = LightingMood::DUSK;
    let mid = shade_window(lit, glass, dusk.uniforms(true), dusk.window_word()[0], true);
    assert!(mid[0] > lit[0] && mid[0] < out[0], "{mid:?}");
}
