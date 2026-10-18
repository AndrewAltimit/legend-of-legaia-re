//! Headless smoke tests: the app state builds without a window, and - with
//! `LEGAIA_DISC_BIN` set - loads the real disc, lists the PROT entries and
//! drives every preview's model and exporter once.

use std::path::PathBuf;

use legaia_disc_audit::categorize::Class;
use legaia_studio::extract_job::{ExtractJob, Outcome};
use legaia_studio::raster::{self, Camera, Model, Shading};
use legaia_studio::{Studio, export};

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("studio-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn disc() -> Option<PathBuf> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)?;
    assert!(p.exists(), "LEGAIA_DISC_BIN={} does not exist", p.display());
    Some(p)
}

#[test]
fn state_builds_without_a_window() {
    let dir = scratch("state");
    let mut s = Studio::new(Some(dir.join("settings.json")));
    assert!(s.library.is_none() && s.blocks.is_empty() && s.selected.is_none());
    assert!(!s.poll());
    // A path that is not a disc is refused at once, with a message.
    s.open(&dir.join("missing.bin"));
    assert!(s.error.is_some() && s.loading().is_none());
    // Settings persist across instances.
    s.settings.skip_png = true;
    s.save_settings();
    assert!(
        Studio::new(Some(dir.join("settings.json")))
            .settings
            .skip_png
    );
}

#[test]
fn extraction_of_a_missing_image_fails_cleanly() {
    let dir = scratch("missing");
    let mut job = ExtractJob::start(legaia_extract::Options {
        bin: dir.join("nope.bin"),
        out: dir.join("out"),
        ..Default::default()
    });
    assert!(matches!(job.join(), Some(Outcome::Failed(_))));
}

#[test]
fn real_disc_lists_prot_entries_and_previews() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping studio disc smoke test");
        return;
    };
    let out = scratch("disc");
    let mut s = Studio::new(Some(out.join("settings.json")));
    s.open_blocking(&bin).expect("load disc");
    let lib = s.library.clone().unwrap();
    assert_eq!(lib.entries.len(), 1233, "PROT.DAT entry count");
    assert!(lib.cdname.is_some(), "CDNAME.TXT read off the disc");
    assert!(
        lib.iso_files
            .iter()
            .any(|f| f.path.eq_ignore_ascii_case("SCUS_942.54"))
    );
    assert!(s.blocks.len() > 50, "entries group under CDNAME blocks");
    assert_eq!(
        s.blocks.iter().map(|b| b.entries.len()).sum::<usize>(),
        1233
    );
    assert_eq!(s.settings.last_source.as_deref(), Some(bin.as_path()));

    // One entry of each preview kind, found by class / content.
    let mut tim = None;
    let mut tmd = None;
    let mut vab = None;
    for pos in 0..lib.entries.len() {
        if tim.is_some() && tmd.is_some() && vab.is_some() {
            break;
        }
        let bytes = lib.entry_bytes(pos).unwrap();
        let class = legaia_disc_audit::categorize::classify(&bytes).class;
        if tim.is_none()
            && legaia_container_formats::tim_scan::scan_entry(&bytes)
                .hits
                .iter()
                .any(|(src, _)| *src == legaia_container_formats::tim_scan::Source::Raw)
        {
            tim = Some(pos);
        }
        if tmd.is_none() && class == Class::SceneTmdStream {
            tmd = Some(pos);
        }
        if vab.is_none() && !legaia_vab::find_vabs(&bytes).is_empty() {
            vab = Some(pos);
        }
    }

    let a = s.select_blocking(tim.expect("a TIM entry")).unwrap();
    assert!(!a.tims.is_empty());
    let png = out.join("tim.png");
    export::tim_png(&a, &a.tims[0], 0, &png).unwrap();
    assert!(std::fs::metadata(&png).unwrap().len() > 0);

    let a = s.select_blocking(tmd.expect("a scene-mesh entry")).unwrap();
    let t = a.tmds.first().expect("TMD found").clone();
    let model = Model::from_tmd(a.tmd_bytes(&t)).expect("drawable TMD");
    let rgba = raster::render(&model, &a.vram(), Camera::default(), Shading::Lit, 160, 120);
    let bg = [30u8, 32, 36, 255];
    assert!(
        rgba.as_chunks::<4>().0.iter().any(|p| *p != bg),
        "the rasteriser drew something"
    );
    export::tmd_obj(&a, &t, &out.join("m.obj")).unwrap();
    export::tmd_glb(&a, &t, None, &out.join("m.glb")).unwrap();
    assert_eq!(&std::fs::read(out.join("m.glb")).unwrap()[..4], b"glTF");

    let a = s.select_blocking(vab.expect("a VAB entry")).unwrap();
    let v = &a.vabs[0];
    let i = (0..v.report.vag_samples.len())
        .find(|&i| v.report.vag_samples[i].size > 0)
        .expect("a non-empty sample");
    export::vag_wav(&a, v, i, &out.join("s.wav")).unwrap();
    assert_eq!(&std::fs::read(out.join("s.wav")).unwrap()[..4], b"RIFF");

    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn real_disc_extraction_cancels() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping studio extraction-cancel test");
        return;
    };
    let out = scratch("cancel");
    let mut job = ExtractJob::start(legaia_extract::Options {
        bin,
        out: out.join("x"),
        skip_verify: true,
        ..Default::default()
    });
    job.cancel();
    assert_eq!(job.join(), Some(Outcome::Cancelled));
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn real_disc_battle_actors_and_scene_textures() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping studio actor test");
        return;
    };
    let out = scratch("actors");
    let mut s = Studio::new(Some(out.join("settings.json")));
    s.open_blocking(&bin).expect("load disc");
    let lib = s.library.clone().unwrap();
    let bg = [30u8, 32, 36, 255];
    let drawn = |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().filter(|p| **p != bg).count();

    // Monsters: named rows, a textured, posed mesh with clips.
    let rows = legaia_studio::actors::monster_rows(&lib).expect("monster rows");
    assert!(rows.len() > 100, "monster archive rows: {}", rows.len());
    let m = legaia_studio::actors::monster(&lib, rows[0].id).expect("first monster");
    assert!(!m.clips.is_empty(), "{} has clips", m.label);
    let img = raster::render(
        &m.model,
        &m.vram,
        Camera::default(),
        Shading::Textured,
        200,
        200,
    );
    assert!(drawn(&img) > 500, "{} drew textured pixels", m.label);
    let _ = legaia_tim::write_png(&out.join("monster.png"), 200, 200, &img);
    let glb = legaia_studio::actors::monster_glb(&lib, rows[0].id).expect("monster glb");
    assert_eq!(&glb[..4], b"glTF");
    let glb = legaia_studio::actors::character_glb(&lib, 0, [0; 5]).expect("character glb");
    assert_eq!(&glb[..4], b"glTF");

    // Party: every character assembles with its default loadout.
    let cats = legaia_studio::actors::character_catalogs(&lib).expect("catalogs");
    assert_eq!(cats.len(), 4);
    for c in &cats {
        assert_eq!(c.sections.len(), 5, "{}: five equipment sections", c.name);
        let a = legaia_studio::actors::character(&lib, c.cslot, [0; 5]).expect("assemble");
        assert!(!a.clips.is_empty(), "{}: clip bank", c.name);
        let img = raster::render(
            &a.model,
            &a.vram,
            Camera::default(),
            Shading::Textured,
            200,
            200,
        );
        assert!(drawn(&img) > 500, "{} drew textured pixels", c.name);
        let _ = legaia_tim::write_png(&out.join(format!("{}.png", c.name)), 200, 200, &img);
    }

    // Scene context: town01's environment pack (entry 4) holds no TIMs of
    // its own; the scene VRAM textures it.
    let pos = lib.position_of(4).unwrap();
    let a = s.select_blocking(pos).unwrap();
    assert!(a.tims.is_empty() && !a.tmds.is_empty());
    let sv = legaia_studio::scene_vram::build(&lib, 4, a.class).expect("scene vram");
    assert!(sv.tims > 0, "scene {} uploaded TIMs", sv.scene);
    let model = Model::from_tmd(a.tmd_bytes(&a.tmds[0])).unwrap();
    let own = raster::render(
        &model,
        &a.vram(),
        Camera::default(),
        Shading::Textured,
        200,
        200,
    );
    let scene = raster::render(
        &model,
        &sv.vram,
        Camera::default(),
        Shading::Textured,
        200,
        200,
    );
    assert!(
        drawn(&scene) > drawn(&own),
        "scene VRAM textures more of the mesh"
    );
    let _ = legaia_tim::write_png(&out.join("scene.png"), 200, 200, &scene);
}
