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

/// The model tab's texture-source default reads coverage. Two `town01`
/// scene_tmd_stream entries each carry two TIMs: PROT 6's cover every CLUT
/// and texel its mesh reads (it opens on the entry), PROT 7's miss a few
/// prims (it opens on the scene's VRAM).
#[test]
fn texture_coverage_separates_self_contained_meshes_from_scene_meshes() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping texture-coverage test");
        return;
    };
    let out = scratch("coverage");
    let mut s = Studio::new(Some(out.join("settings.json")));
    s.open_blocking(&bin).expect("load disc");
    let lib = s.library.clone().unwrap();
    let mut coverage = |index: u32| {
        let pos = lib.position_of(index).expect("entry");
        let a = s.select_blocking(pos).expect("analyse");
        assert!(!a.tims.is_empty(), "PROT {index} carries TIMs of its own");
        let m = Model::from_tmd(a.tmd_bytes(&a.tmds[0])).expect("mesh builds");
        m.texture_coverage(&a.vram())
    };
    // PROT 7: three triangles read a page the scene uploads.
    let (covered, textured) = coverage(7);
    assert!(
        textured > 0 && covered < textured,
        "PROT 7: {covered}/{textured}"
    );
    // PROT 6: its two TIMs hold everything its mesh reads.
    let (covered, textured) = coverage(6);
    assert!(
        textured > 0 && covered == textured,
        "PROT 6: {covered}/{textured}"
    );
}

/// PROT 6 (`town01`'s dark-sky shell and island) draws almost black with
/// retail shading although its own TIMs cover every page it reads: its sky
/// dome is untextured gouraud authored `(0, 0, 0)` at every corner and most
/// of the island's colour words sit near black over a dim palette. The
/// model tab says so (`dark_baked_share`), and the Lit aid still shows its
/// shape. PROT 7, the daytime twin with a blue dome, carries none.
#[test]
fn a_mesh_baked_black_on_the_disc_is_named_and_lit_shows_its_shape() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping baked-dark test");
        return;
    };
    let out = scratch("baked-dark");
    let mut s = Studio::new(Some(out.join("settings.json")));
    s.open_blocking(&bin).expect("load disc");
    let lib = s.library.clone().unwrap();
    let mut probe = |index: u32| {
        let a = s
            .select_blocking(lib.position_of(index).expect("entry"))
            .expect("analyse");
        let m = Model::from_tmd(a.tmd_bytes(&a.tmds[0])).expect("mesh builds");
        let near_black = |shading| {
            let img = raster::render(&m, &a.vram(), Camera::default(), shading, 160, 120);
            let px: Vec<_> = img
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] != [30, 32, 36])
                .map(|p| p[0].max(p[1]).max(p[2]) < 16)
                .collect();
            (px.iter().filter(|&&b| b).count(), px.len().max(1))
        };
        // The asset viewer measures the raw TMD; both tools read one number.
        let bytes = a.tmd_bytes(&a.tmds[0]);
        let whole = legaia_tmd::mesh::tmd_dark_baked_share(
            &legaia_tmd::parse(bytes).expect("parse"),
            bytes,
        );
        assert!((whole - m.dark_baked_share()).abs() < 1e-6, "PROT {index}");
        (
            m.dark_baked_share(),
            near_black(Shading::Textured),
            near_black(Shading::Lit),
        )
    };
    let (dark6, (tex_black, tex_drawn), (lit_black, _)) = probe(6);
    assert!(
        dark6 >= legaia_tmd::mesh::MOSTLY_DARK_SHARE,
        "PROT 6 dark share {dark6}"
    );
    assert!(
        tex_black * 2 > tex_drawn,
        "PROT 6 draws mostly black with retail shading ({tex_black}/{tex_drawn})"
    );
    assert_eq!(lit_black, 0, "the Lit aid floors black prims to a grey");
    let (dark7, _, _) = probe(7);
    assert!(dark7 < 0.05, "PROT 7 dark share {dark7}");
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
    assert_eq!(
        s.settings.recent.first(),
        Some(&bin),
        "opened disc is recent"
    );

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
    // Names read as the font draws them: the `$N` variant suffix is the
    // level mark, and the element escape is a badge, not text.
    assert!(rows.iter().any(|r| r.name.ends_with(" Lv2")));
    assert!(
        rows.iter()
            .all(|r| !r.name.contains('$') && !r.name.contains('^'))
    );
    assert!(rows.iter().any(|r| r.badge == Some("Fire")));
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
        // Vahn, Noa and Gala carry their Tactical Arts as named clips.
        let arts = a
            .clips
            .iter()
            .filter(|k| k.label.starts_with("Art: "))
            .count();
        if c.cslot < 3 {
            assert!(arts >= 15, "{}: {arts} art clips", c.name);
        }
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
    // The play host's two layers: the field-character atlas and the
    // effect-texture pool.
    assert_eq!(sv.layers.len(), 2, "{:?}", sv.layers);
    assert!(
        sv.layers.iter().all(|l| !l.contains("skipped")),
        "{:?}",
        sv.layers
    );
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

#[test]
fn real_disc_search_and_streamed_media() {
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping studio media test");
        return;
    };
    let out = scratch("media");
    let mut s = Studio::new(Some(out.join("settings.json")));
    s.open_blocking(&bin).expect("load disc");
    let lib = s.library.clone().unwrap();

    // Search: CDNAME label, retail block and index forms all match.
    let pos = lib.position_of(865).unwrap();
    let block = lib.label(865).unwrap().to_string();
    for q in ["865", "0865", "0x361", "battle", "BATTLE 865"] {
        assert!(s.entry_matches(pos, &block, q, None), "query {q:?}");
    }
    assert!(!s.entry_matches(pos, &block, "town01", None));
    // A scene's display name finds its block: "rim elm" is town01.
    let town = lib.position_of(5).unwrap();
    let town_block = lib.label(5).unwrap().to_string();
    assert_eq!(legaia_studio::place_name(&town_block), Some("Rim Elm"));
    assert!(s.entry_matches(town, &town_block, "rim elm", None));
    assert!(!s.entry_matches(pos, &block, "rim elm", None));

    // CD-XA: the first file's channels decode at their own subheader rate.
    let xa = lib
        .iso_files
        .iter()
        .find(|f| {
            legaia_studio::media::kind_of(&f.path) == Some(legaia_studio::media::MediaKind::Xa)
        })
        .expect("an XA file");
    let chans = legaia_studio::media::xa_channels(&bin, xa).expect("demux");
    assert!(!chans.is_empty(), "{} has channels", xa.path);
    let t = legaia_studio::media::decode_channel(&chans[0]).expect("decode");
    assert!(t.duration_secs() > 1.0 && t.pcm.iter().any(|&s| s != 0));
    legaia_studio::media::write_wav(&out.join("xa.wav"), &t.pcm, t.channels, t.sample_rate)
        .expect("wav");

    // STR: the smallest movie assembles frames that decode, at 15 fps, with
    // a soundtrack.
    let mv = lib
        .iso_files
        .iter()
        .filter(|f| {
            legaia_studio::media::kind_of(&f.path) == Some(legaia_studio::media::MediaKind::Str)
        })
        .min_by_key(|f| f.size)
        .expect("a movie");
    let m = legaia_studio::media::str_movie(&bin, mv).expect("demux movie");
    assert!(
        m.frames.len() > 30,
        "{}: {} frames",
        mv.path,
        m.frames.len()
    );
    assert!((m.frame_period_secs() - 1.0 / 15.0).abs() < 1e-3);
    assert!(m.audio.is_some(), "{} soundtrack", mv.path);
    let (w, h) = m.size();
    let rgba = m.frames[m.frames.len() / 2]
        .decode_rgba()
        .expect("frame decodes");
    assert_eq!(rgba.len(), (w * h * 4) as usize);
}

/// Equipment names come from the opened build's own executable: the USA
/// read through `from_exe` is the same table as `from_scus`, and a PAL disc
/// (`LEGAIA_PAL_DISC_BIN`, optional) names its items, accents included.
#[test]
fn equipment_names_per_build() {
    use legaia_game_tables::item_names::ItemNameTable;
    let Some(bin) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN not set; skipping equipment-name test");
        return;
    };
    let usa = legaia_studio::Library::open(&bin).expect("load USA disc");
    let scus = usa.executable().expect("SCUS");
    assert_eq!(
        ItemNameTable::from_exe(&scus, "SCUS_942.54"),
        ItemNameTable::from_scus(&scus)
    );
    let Some(pal) = std::env::var_os("LEGAIA_PAL_DISC_BIN")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    else {
        eprintln!("[skip] LEGAIA_PAL_DISC_BIN not set; USA half only");
        return;
    };
    // The same options go unnamed on both builds (the table's empty slots).
    let unnamed = |lib: &legaia_studio::Library| -> Vec<String> {
        legaia_studio::actors::character_catalogs(lib)
            .expect("catalogs")
            .iter()
            .flat_map(|c| c.sections.iter().flat_map(|(_, ids)| ids.iter()))
            .filter(|(id, n)| *id != 0 && n.starts_with("id "))
            .map(|(_, n)| n.clone())
            .collect()
    };
    let lib = legaia_studio::Library::open(&pal).expect("load PAL disc");
    assert_eq!(
        unnamed(&lib),
        unnamed(&usa),
        "PAL options named as on the USA disc"
    );
    let exe = lib.executable().expect("PAL executable");
    let table = ItemNameTable::from_exe(&exe, lib.exe.as_deref().unwrap()).expect("PAL table");
    assert!(
        (0..=255u8).any(|i| table.name(i).is_some_and(|n| !n.is_ascii())),
        "some PAL item name carries an accented letter"
    );
}
