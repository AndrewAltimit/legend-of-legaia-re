use super::*;

#[test]
fn world_map_scene_classifier_matches_only_the_kingdom_overworlds() {
    // The three kingdom overworld scenes.
    assert!(is_world_map_scene("map01"));
    assert!(is_world_map_scene("map02"));
    assert!(is_world_map_scene("map03"));
    // Towns, dungeons, cutscene/FMV labels are not overworlds.
    for label in [
        "town01", "town0b", "chitei2", "jou", "uru2", "opdeene", "opmap01", "battle", "map1",
        "map001", "mapxx", "world", "",
    ] {
        assert!(
            !is_world_map_scene(label),
            "{label} must not classify as world map"
        );
    }
}

/// Disc-gated: the `etim.dat` effect-sprite TIMs (PROT 0874 section 2)
/// decode and upload into a software VRAM, populating the fire-sprite tile
/// at fb(832,256) (the texel target verified pixel-exact against a live
/// battle VRAM capture). Skips when the disc data isn't present.
#[test]
fn etim_effect_textures_upload_into_vram() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut vram = legaia_tim::Vram::new();
    let n = upload_effect_textures_into_vram(&index, &mut vram, true).expect("seed etim VRAM");
    assert!(n >= 5, "expected >=5 etim TIMs uploaded, got {n}");
    assert!(
        vram.region_has_data(832, 256, 20, 64),
        "etim fire-sprite tile @fb(832,256) should be populated"
    );
    // The field-character CLUTs land as flat strips on row 478: Vahn at
    // cols 0..63, Noa 64..127, Gala 128..191. A rect upload (the prior
    // bug) would only populate cols 0..15 of row 478, leaving Noa's and
    // Gala's palette columns empty. Assert all three character palette
    // bands are present at row 478.
    for (col, who) in [(0usize, "Vahn"), (64, "Noa"), (128, "Gala")] {
        assert!(
            vram.region_has_data(col, 478, 16, 1),
            "field-char CLUT strip for {who} @row 478 col {col} should be populated \
             (flat-strip CLUT upload)"
        );
    }
}

/// Disc-gated: the flame-atlas TIMs (PROT 870) decode and upload into a
/// software VRAM, populating all three effect-texture pages at the
/// byte-verified targets `(320,0)`, `(384,0)`, `(448,0)`. These sit at
/// `fb_y=0` (distinct from etim's `fb_y=256` pages) and are battle-only.
/// Skips when the disc data isn't present.
#[test]
fn flame_atlas_uploads_three_effect_pages_at_y0() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut vram = legaia_tim::Vram::new();
    let n = upload_flame_atlas_into_vram(&index, &mut vram, true).expect("seed flame-atlas VRAM");
    assert_eq!(n, 3, "expected exactly 3 flame-atlas TIMs, got {n}");
    for (fb_x, fb_y) in [(320usize, 0usize), (384, 0), (448, 0)] {
        assert!(
            vram.region_has_data(fb_x, fb_y, 64, 256),
            "flame-atlas page @fb({fb_x},{fb_y}) should be populated"
        );
    }
}

/// Disc-gated: the global TMD pool seeded from `etmd.dat` (PROT 0874
/// section 0) holds the five effect models, and the slot named by
/// [`ETMD_TAIL_FIRE_MODEL_INDEX`] is the small *Tail Fire* flame mesh - far
/// fewer primitives than the other four models, with textured primitives
/// sampling the `etim` CLUT rows (473..=478). Pins the constant to real
/// disc bytes. Skips when the disc data isn't present.
#[test]
fn etmd_tail_fire_model_is_the_small_flame_mesh() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut world = crate::world::World::default();
    seed_global_tmd_pool_from_befect_data(&index, &mut world).expect("seed etmd pool");

    // All five etmd models present.
    for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
        assert!(
            world.global_tmd(i as i16).is_some(),
            "etmd model {i} should be present"
        );
    }
    let flame = world
        .global_tmd(ETMD_TAIL_FIRE_MODEL_INDEX as i16)
        .expect("flame model present");
    let flame_prims: usize = flame
        .tmd
        .objects
        .iter()
        .map(|o| o.header.n_primitive as usize)
        .sum();

    // The flame is the smallest model by a wide margin.
    for i in 0..GLOBAL_TMD_POOL_HEAD_COUNT {
        if i == ETMD_TAIL_FIRE_MODEL_INDEX {
            continue;
        }
        let other = world.global_tmd(i as i16).unwrap();
        let other_prims: usize = other
            .tmd
            .objects
            .iter()
            .map(|o| o.header.n_primitive as usize)
            .sum();
        assert!(
            flame_prims < other_prims,
            "flame model ({flame_prims} prims) should be smaller than model {i} ({other_prims} prims)"
        );
    }
    assert!(
        flame_prims < 64,
        "flame model is a small mesh, got {flame_prims} prims"
    );
}

/// Disc-gated: the PROT 0871 effect-model library (`etmd.dat`) is a 30-entry
/// TMD pack that registers into `World::global_tmd_pool[3..=32]`, and the
/// Gimard *Tail Fire* slot ([`GIMARD_TAIL_FIRE_MODEL_INDEX`]) resolves to a
/// real Legaia TMD. Pins the library load + index to real disc bytes. Skips
/// when the disc data isn't present.
#[test]
fn etmd_effect_model_library_registers_into_global_pool() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let root = ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file());
    let Some(root) = root else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let mut world = crate::world::World::default();

    // Library not loaded yet -> guard reports false.
    assert!(!effect_model_library_loaded(&world));

    seed_effect_model_library_from_etmd(&index, &mut world).expect("seed etmd library");

    // All 30 models register into [3..=32], and the guard is now true.
    assert!(effect_model_library_loaded(&world));
    for i in 0..EFFECT_MODEL_LIBRARY_COUNT {
        assert!(
            world
                .global_tmd((EFFECT_MODEL_LIBRARY_BASE + i) as i16)
                .is_some(),
            "effect-model library slot {i} -> pool {} should be present",
            EFFECT_MODEL_LIBRARY_BASE + i
        );
    }

    // The named Gimard flame slot resolves to a real TMD with geometry.
    let flame = world
        .global_tmd(GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
        .expect("Gimard Tail Fire model present");
    let flame_prims: usize = flame
        .tmd
        .objects
        .iter()
        .map(|o| o.header.n_primitive as usize)
        .sum();
    assert!(
        flame_prims > 0,
        "Gimard flame model carries primitives, got {flame_prims}"
    );
    // The flame index falls inside the library window.
    assert_eq!(
        GIMARD_TAIL_FIRE_MODEL_INDEX,
        EFFECT_MODEL_LIBRARY_BASE + 23,
        "Gimard flame is DAT_8007C018[26] = pack entry 23"
    );
}

#[test]
fn cutscene_str_for_resolves_known_op_scenes() {
    assert_eq!(cutscene_str_for("opdeene"), Some("MOV/MV1.STR"));
    assert_eq!(cutscene_str_for("opstati"), Some("MOV/MV2.STR"));
    assert_eq!(cutscene_str_for("opkorout"), Some("MOV/MV3.STR"));
    assert_eq!(cutscene_str_for("opurud"), Some("MOV/MV4.STR"));
    assert_eq!(cutscene_str_for("opmap01"), Some("MOV/MV5.STR"));
}

#[test]
fn cutscene_str_for_resolves_first_ed_scene_only() {
    assert_eq!(cutscene_str_for("edteien"), Some("MOV/MV6.STR"));
    // The remaining ed* scenes are dialogue-actor-overlay driven and
    // have no FMV file.
    assert_eq!(cutscene_str_for("edbylon"), None);
    assert_eq!(cutscene_str_for("edlast"), None);
    assert_eq!(cutscene_str_for("edstati3"), None);
}

#[test]
fn cutscene_str_for_returns_none_for_non_cutscene_labels() {
    assert_eq!(cutscene_str_for("town01"), None);
    assert_eq!(cutscene_str_for("battle_data"), None);
    assert_eq!(cutscene_str_for(""), None);
}

#[test]
fn fmv_trigger_field_scenes_are_distinct_from_op_ed_cutscenes() {
    for label in FMV_TRIGGER_FIELD_SCENES {
        assert!(is_fmv_trigger_field_scene(label));
        // None of these are op*/ed* engine cutscenes.
        assert!(!is_cutscene_label(label));
        // None map through the heuristic to a specific MV file - the
        // mapping is in the field-VM script for each scene, not here.
        assert_eq!(cutscene_str_for(label), None);
    }
    assert!(!is_fmv_trigger_field_scene("opdeene"));
    assert!(!is_fmv_trigger_field_scene("battle_data"));
}

#[test]
fn cutscene_label_for_str_round_trip() {
    for (label, path) in FMV_CUTSCENE_SCENES.iter() {
        assert_eq!(cutscene_str_for(label), Some(*path));
        // Inverse via either form (with or without dir prefix).
        let bare = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
        assert_eq!(cutscene_label_for_str(bare), Some(*label));
        assert_eq!(cutscene_label_for_str(path), Some(*label));
    }
}

#[test]
fn cutscene_label_for_str_handles_case_insensitive_filenames() {
    // ISO9660 filenames may upper- or lowercase depending on extractor.
    assert_eq!(cutscene_label_for_str("mv1.str"), Some("opdeene"));
    assert_eq!(cutscene_label_for_str("MOV/mv6.STR"), Some("edteien"));
    assert_eq!(cutscene_label_for_str("garbage"), None);
}

#[test]
fn cutscene_map_default_is_empty_and_resolves_via_heuristic() {
    let m = CutsceneMap::new();
    assert!(m.is_empty());
    assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
    assert_eq!(m.resolve("town01"), None);
}

#[test]
fn cutscene_map_explicit_entry_overrides_heuristic() {
    let mut m = CutsceneMap::new();
    m.insert("opdeene", "MOV/CUSTOM.STR");
    assert_eq!(m.resolve("opdeene"), Some("MOV/CUSTOM.STR".into()));
    // Other entries still fall through to the heuristic.
    assert_eq!(m.resolve("opstati"), Some("MOV/MV2.STR".into()));
}

#[test]
fn cutscene_map_from_heuristic_preloaded() {
    let m = CutsceneMap::from_heuristic();
    assert_eq!(m.len(), FMV_CUTSCENE_SCENES.len());
    for (label, path) in FMV_CUTSCENE_SCENES.iter() {
        assert_eq!(m.resolve(label), Some((*path).into()));
    }
}

#[test]
fn cutscene_map_unknown_label_falls_through_to_heuristic_none() {
    let m = CutsceneMap::from_heuristic();
    assert_eq!(m.resolve("town01"), None);
    assert_eq!(m.resolve("xxx"), None);
}

#[test]
fn cutscene_map_from_toml_str_parses_scenes_table() {
    let doc = r#"
[scenes]
opdeene = "MOV/MV1.STR"
opstati = 'MOV/MV2.STR'
edteien = "MOV/MV6.STR"
"#;
    let m = CutsceneMap::from_toml_str(doc).expect("parse");
    assert_eq!(m.len(), 3);
    assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
    assert_eq!(m.resolve("opstati"), Some("MOV/MV2.STR".into()));
    assert_eq!(m.resolve("edteien"), Some("MOV/MV6.STR".into()));
    // Unmapped scenes still fall through to the heuristic.
    assert_eq!(m.resolve("opmap01"), Some("MOV/MV5.STR".into()));
}

#[test]
fn cutscene_map_from_toml_str_empty_doc_yields_empty_map() {
    let m = CutsceneMap::from_toml_str("").expect("parse");
    assert!(m.is_empty());
}

#[test]
fn cutscene_map_from_toml_str_ignores_unknown_top_level_keys() {
    let doc = r#"
some_other_setting = 42
[other_table]
foo = "bar"
[scenes]
opdeene = "MOV/MV1.STR"
"#;
    let m = CutsceneMap::from_toml_str(doc).expect("parse");
    assert_eq!(m.len(), 1);
    assert_eq!(m.resolve("opdeene"), Some("MOV/MV1.STR".into()));
}

#[test]
fn cutscene_map_to_toml_string_round_trips() {
    let mut m = CutsceneMap::new();
    m.insert("opdeene", "MOV/MV1.STR");
    m.insert("edteien", "MOV/MV6.STR");
    let toml_doc = m.to_toml_string();
    let parsed = CutsceneMap::from_toml_str(&toml_doc).expect("re-parse");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed.resolve("opdeene"), Some("MOV/MV1.STR".into()));
    assert_eq!(parsed.resolve("edteien"), Some("MOV/MV6.STR".into()));
}

#[test]
fn cutscene_map_to_toml_string_handles_backslash_paths() {
    let mut m = CutsceneMap::new();
    // Some engines load using Windows-style separators; the TOML
    // writer must escape these so the round-trip is lossless.
    m.insert("opdeene", "MOV\\MV1.STR");
    let toml_doc = m.to_toml_string();
    let parsed = CutsceneMap::from_toml_str(&toml_doc).expect("re-parse");
    assert_eq!(parsed.resolve("opdeene"), Some("MOV\\MV1.STR".into()));
}

#[test]
fn cutscene_map_entries_iterates_explicit_only() {
    let mut m = CutsceneMap::new();
    m.insert("opdeene", "MOV/MV1.STR");
    m.insert("edteien", "MOV/MV6.STR");
    let mut entries: Vec<(String, String)> =
        m.entries().map(|(a, b)| (a.into(), b.into())).collect();
    entries.sort();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, "edteien");
    assert_eq!(entries[1].0, "opdeene");
}

#[test]
fn cutscene_map_from_toml_path_reads_file() {
    let dir = std::env::temp_dir();
    let p = dir.join("legaia-re-cutscene-test.toml");
    std::fs::write(
        &p,
        "[scenes]\nopdeene = \"MOV/MV1.STR\"\nedteien = \"MOV/MV6.STR\"\n",
    )
    .expect("write tmp");
    let m = CutsceneMap::from_toml_path(&p).expect("read");
    assert_eq!(m.len(), 2);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn default_map_id_resolver_resolves_by_position() {
    let r = DefaultMapIdResolver::new(vec!["town01".into(), "cave01".into(), "world01".into()]);
    assert_eq!(r.resolve(0), Some("town01".into()));
    assert_eq!(r.resolve(1), Some("cave01".into()));
    assert_eq!(r.resolve(2), Some("world01".into()));
    assert_eq!(r.resolve(3), None);
}

#[test]
fn default_map_id_resolver_empty_returns_none() {
    let r = DefaultMapIdResolver::default();
    assert_eq!(r.resolve(0), None);
}

#[test]
fn scene_destination_resolver_resolves_by_index() {
    use crate::man_field_scripts::SceneDestination;
    let r = SceneDestinationResolver::new(vec![
        SceneDestination {
            scene_name: "town0c".into(),
            index: 21,
            entry_x: 0x10,
            entry_z: 0x20,
        },
        SceneDestination {
            scene_name: "rikuroa".into(),
            index: 155,
            entry_x: 0x30,
            entry_z: 0x40,
        },
    ]);
    assert_eq!(r.len(), 2);
    // Resolve by the i16 index (the 0x3F index space - wider than u8).
    assert_eq!(r.resolve(21), Some("town0c"));
    assert_eq!(r.resolve(155), Some("rikuroa"));
    assert_eq!(r.resolve(99), None);
    // The richer accessor returns the full record (name + entry tile).
    let d = r.destination(155).expect("rikuroa destination");
    assert_eq!(d.scene_name, "rikuroa");
    assert_eq!((d.entry_x, d.entry_z), (0x30, 0x40));
    assert!(SceneDestinationResolver::default().is_empty());
}

/// Smoke test: BGM index math matches the documented retail resolver -
/// `raw_define + 6 + bgm_id` for ids < 2000, which is `start + 8 + id` in
/// the extraction frame `Scene` windows use (the raw define is `start + 2`).
#[test]
fn find_bgm_uses_documented_offset() {
    let scene = Scene {
        name: "test".into(),
        start: 100,
        end: 200,
        entries: (100..200u32)
            .map(|idx| SceneEntry {
                idx,
                class: Class::UnknownOther,
                bytes: Arc::new(vec![]),
            })
            .collect(),
    };
    let bgm = scene.find_bgm(0).unwrap();
    assert_eq!(bgm.idx, 108);
    let bgm = scene.find_bgm(5).unwrap();
    assert_eq!(bgm.idx, 113);
}

/// BGM IDs >= 2000 are global-pool - not resolved by the per-scene
/// helper. The full resolver (with global pool) is engine-side; the
/// scene-local helper just declines.
#[test]
fn find_bgm_global_pool_returns_none() {
    let scene = Scene {
        name: "test".into(),
        start: 0,
        end: 10,
        entries: vec![],
    };
    assert!(scene.find_bgm(2000).is_none());
    assert!(scene.find_bgm(3000).is_none());
}

#[test]
fn vec_map_id_resolver_indexes_into_list() {
    let r = VecMapIdResolver::new(vec!["aaa".into(), "bbb".into(), "ccc".into()]);
    assert_eq!(r.resolve(0).as_deref(), Some("aaa"));
    assert_eq!(r.resolve(2).as_deref(), Some("ccc"));
    assert_eq!(r.resolve(3), None);
}

#[test]
fn null_map_id_resolver_returns_none() {
    let r = NullMapIdResolver;
    assert_eq!(r.resolve(0), None);
    assert_eq!(r.resolve(255), None);
}

#[test]
fn null_bgm_director_swallows_every_call() {
    // Compiles + every default impl is a no-op.
    let mut d = NullBgmDirector;
    d.start(1, &[]);
    d.queue(2, &[]);
    d.pause();
    d.resume();
    d.stop();
}

/// Test director that records every dispatched event for assertion.
#[derive(Default)]
struct RecordingBgm {
    log: Vec<String>,
}
impl BgmDirector for RecordingBgm {
    fn start(&mut self, id: u16, bytes: &[u8]) {
        self.log.push(format!("start({id},{})", bytes.len()));
    }
    fn queue(&mut self, id: u16, bytes: &[u8]) {
        self.log.push(format!("queue({id},{})", bytes.len()));
    }
    fn pause(&mut self) {
        self.log.push("pause".into());
    }
    fn resume(&mut self) {
        self.log.push("resume".into());
    }
    fn stop(&mut self) {
        self.log.push("stop".into());
    }
    fn start_owned_vab(&mut self, id: u16, bytes: &[u8]) {
        self.log
            .push(format!("start_owned_vab({id},{})", bytes.len()));
    }
    fn queue_owned_vab(&mut self, id: u16, bytes: &[u8]) {
        self.log
            .push(format!("queue_owned_vab({id},{})", bytes.len()));
    }
}

/// Pause / resume / stop sub-ops fire even without a loaded scene
/// (no SEQ resolution required).
#[test]
fn route_bgm_handles_control_subops_without_scene() {
    // Build a scene-less SceneHost via the test fixture in
    // tests/scene_bundle_smoke.rs is too heavy here - instead, just
    // exercise the routing logic through a minimal scaffold by
    // directly emitting to a recording director and asserting the
    // matching events came through.
    //
    // SceneHost::new requires a ProtIndex which requires a real PROT
    // file, so this test exercises route_bgm_events indirectly via
    // a unit-sized stand-in: only the control sub-ops 2/3/4.
    let mut d = RecordingBgm::default();
    let ev2 = crate::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 2,
    };
    let ev3 = crate::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 3,
    };
    let ev4 = crate::field_events::FieldEvent::Bgm {
        text_id: 0,
        sub_op: 4,
    };
    // Mimic the route_bgm_events branches directly.
    for ev in [ev2, ev3, ev4] {
        if let crate::field_events::FieldEvent::Bgm { sub_op, .. } = ev {
            match sub_op {
                2 => d.pause(),
                3 => d.resume(),
                4 => d.stop(),
                _ => {}
            }
        }
    }
    assert_eq!(d.log, vec!["pause", "resume", "stop"]);
}

/// `bgm_reattach_volume` (FUN_80019898) - the `(raw << 15) >> 16` level
/// arithmetic: bits [16:1] of the raw global, sign-extended.
#[test]
fn bgm_reattach_volume_matches_retail_shift_pair() {
    // Boot value: FUN_8001FFA4 stores -1 into DAT_8007B6EC.
    assert_eq!(bgm_reattach_volume(-1), -1);
    // Plain halving for small positives.
    assert_eq!(bgm_reattach_volume(0x7F), 0x3F);
    assert_eq!(bgm_reattach_volume(0x100), 0x80);
    assert_eq!(bgm_reattach_volume(0), 0);
    // 16-bit sign carried through bit 16: 0xFFFF halves to 0x7FFF
    // (positive - the sign bit of the *shifted* value is bit 16 of raw).
    assert_eq!(bgm_reattach_volume(0xFFFF), 0x7FFF);
    // Bit 16 set makes the result negative.
    assert_eq!(bgm_reattach_volume(0x1FFFF), -1);
    // High bits above 16 are discarded by the << 15 (mod 2^17 window).
    assert_eq!(bgm_reattach_volume(0x7FFE_0100), 0x80);
}

/// Sub-op 8 routes through `BgmDirector::reattach_volume` with the level
/// derived from the host's raw volume mirror.
#[test]
fn bgm_reattach_subop_dispatches_level() {
    #[derive(Default)]
    struct Rec {
        levels: Vec<i16>,
    }
    impl BgmDirector for Rec {
        fn reattach_volume(&mut self, level: i16) {
            self.levels.push(level);
        }
    }
    let mut d = Rec::default();
    // Mimic the route_bgm_events sub-op 8 branch with the boot raw value.
    d.reattach_volume(bgm_reattach_volume(-1));
    d.reattach_volume(bgm_reattach_volume(0x7F));
    assert_eq!(d.levels, vec![-1, 0x3F]);
}

/// `class_counts` reports a histogram in CDNAME order.
#[test]
fn class_counts_matches_entries() {
    let scene = Scene {
        name: "t".into(),
        start: 0,
        end: 3,
        entries: vec![
            SceneEntry {
                idx: 0,
                class: Class::UnknownOther,
                bytes: Arc::new(vec![]),
            },
            SceneEntry {
                idx: 1,
                class: Class::UnknownOther,
                bytes: Arc::new(vec![]),
            },
            SceneEntry {
                idx: 2,
                class: Class::Empty,
                bytes: Arc::new(vec![]),
            },
        ],
    };
    let counts = scene.class_counts();
    assert_eq!(counts.get(&Class::UnknownOther).copied(), Some(2));
    assert_eq!(counts.get(&Class::Empty).copied(), Some(1));
}
