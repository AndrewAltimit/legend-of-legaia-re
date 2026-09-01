//! `export-glb`: bake a scene (or every scene) into the textured `.glb` +
//! manifest set for Unity / VRChat world building. Thin driver over the
//! shared kernels: `engine-core::scene_assembly` (the same assembly the
//! browser field-scene page renders) + `engine-core::glb_export` (the
//! composition + baking). See `docs/tooling/vrchat-world-export.md`.

use super::*;
use legaia_engine_core::glb_export::{
    FloorSampler, GlbExportOptions, export_animated_prop_glbs, export_equipment_item_glbs,
    export_npc_glbs, export_scene_traversal, export_world_glb, items_manifest, world_manifest,
};
use legaia_engine_core::npc_catalog::catalog_scene_npcs;
use legaia_engine_core::scene_assembly::assemble_field_scene;

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_export_glb(
    scenes: &[String],
    all_scenes: bool,
    out: &Path,
    scale: f32,
    include_sky: bool,
    no_npcs: bool,
    no_props: bool,
    items: bool,
    extracted_root: &Path,
    disc: Option<&Path>,
) -> Result<()> {
    if scenes.is_empty() && !all_scenes && !items {
        anyhow::bail!("pass --scene <name> (repeatable), --all-scenes, or --items");
    }
    let index = open_index_from_args(extracted_root, disc)?;
    if items {
        export_items(&index, out, extracted_root, disc)?;
        if scenes.is_empty() && !all_scenes {
            println!(
                "note: the output contains Sony-derived game data - keep it local, never redistribute it"
            );
            return Ok(());
        }
    }
    let names: Vec<String> = if all_scenes {
        index
            .cdname_scene_names()
            .into_iter()
            .filter(|n| !legaia_engine_core::scene::is_cutscene_label(n))
            .collect()
    } else {
        scenes.to_vec()
    };
    if names.is_empty() {
        anyhow::bail!("no scene names resolved (is CDNAME.TXT present?)");
    }
    let opts = GlbExportOptions { scale, include_sky };
    let mut exported = 0usize;
    let mut skipped = 0usize;
    for name in &names {
        match export_one(&index, name, out, &opts, no_npcs, no_props) {
            Ok(true) => exported += 1,
            Ok(false) => skipped += 1,
            Err(e) if all_scenes => {
                skipped += 1;
                eprintln!("  [skip] {name}: {e:#}");
            }
            Err(e) => return Err(e.context(format!("export scene {name}"))),
        }
    }
    println!(
        "export-glb: {exported} scene(s) exported, {skipped} skipped -> {}",
        out.display()
    );
    println!(
        "note: the output contains Sony-derived game data - keep it local, never redistribute it"
    );
    Ok(())
}

/// Export one scene. `Ok(false)` = assembled but nothing drawable.
fn export_one(
    index: &ProtIndex,
    name: &str,
    out: &Path,
    opts: &GlbExportOptions,
    no_npcs: bool,
    no_props: bool,
) -> Result<bool> {
    let a = assemble_field_scene(index, name).map_err(|e| anyhow::anyhow!(e))?;
    let scene = Scene::load(index, name)?;
    let world = export_world_glb(index, &scene, &a, opts).map_err(|e| anyhow::anyhow!(e))?;
    if world.glb.is_empty() {
        println!("  [empty] {name}: no drawable geometry");
        return Ok(false);
    }
    let dir = out.join(name);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let world_file = format!("{name}.glb");
    std::fs::write(dir.join(&world_file), &world.glb)?;

    // NPC catalog is best-effort: a world-map scene has no MAN placements.
    let catalog = if no_npcs {
        None
    } else {
        catalog_scene_npcs(index, name, &a.res, None).ok()
    };
    let npcs = match catalog.as_ref() {
        Some(c) => export_npc_glbs(&scene, &a, c),
        None => Vec::new(),
    };
    if !npcs.is_empty() {
        let npc_dir = dir.join("npcs");
        std::fs::create_dir_all(&npc_dir)?;
        for n in &npcs {
            std::fs::write(npc_dir.join(format!("{}.glb", n.file_stem)), &n.glb)?;
        }
    }
    let props = if no_props {
        Vec::new()
    } else {
        export_animated_prop_glbs(&scene, &a, opts)
    };
    if !props.is_empty() {
        let prop_dir = dir.join("props");
        std::fs::create_dir_all(&prop_dir)?;
        for p in &props {
            std::fs::write(prop_dir.join(format!("{}.glb", p.file_stem)), &p.glb)?;
        }
    }
    let floor = FloorSampler::build(index, &scene);
    let traversal = export_scene_traversal(index, &scene, &floor, opts);
    let mut manifest = world_manifest(
        &a,
        opts,
        &world,
        &world_file,
        catalog.as_ref(),
        &npcs,
        &props,
        &floor,
        &traversal,
    );
    // Scene music: render the MAN's opening BGM start through the engine's
    // SPU + sequencer to a seamlessly-looping WAV (Sony-derived output, same
    // rules as the glbs). Soft-fails - a scene without a resolvable track
    // just ships no `music` block.
    manifest["music"] = match export_scene_music(index, &scene, &dir) {
        Some(m) => m,
        None => serde_json::Value::Null,
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    println!(
        "  [ok] {name}: {} meshes, {} instances ({} sky hidden), {} ground quads, \
         {} NPC glb(s), {} animated prop glb(s)",
        world.mesh_count,
        world.instance_count,
        world.sky_hidden,
        world.ground_quads,
        npcs.len(),
        props.len(),
    );
    Ok(true)
}

/// Render the scene's entry BGM to `music/bgm_<id>.wav` and return the
/// manifest `music` block. The track is the scene MAN's **first** op-`0x35`
/// sub-1 BGM start (the id the controller script plays at entry -
/// `man_field_scripts::scene_bgm_starts`), resolved the way the play hosts
/// resolve it: global ids (`>= 2000`) through the `music_01` bank map
/// ([`legaia_engine_core::music_labels::prot_entry_for_bgm_id`]), scene-local
/// ids through the scene's own SEQ/VAB entries.
///
/// The render drives the engine's SPU + SsAPI sequencer sample-by-sample
/// ([`legaia_engine_audio::render_bgm_loop_region`], the site's minigame-BGM
/// path) and writes ONLY the detected loop region, so an `AudioSource` set to
/// loop plays it seamlessly with no lead-in seam. `None` when the scene names
/// no track or the pair doesn't parse.
fn export_scene_music(index: &ProtIndex, scene: &Scene, dir: &Path) -> Option<serde_json::Value> {
    use legaia_engine_audio::{SPU_INTERNAL_RATE, render_bgm_loop_region};
    use serde_json::json;

    let man = scene.field_man_payload(index).ok()??;
    let mf = legaia_asset::man_section::parse(&man).ok()?;
    let starts = legaia_engine_core::man_field_scripts::scene_bgm_starts(&mf, &man);
    let bgm_id = starts.first()?.bgm_id;

    // Resolve the [VAB..][SEQ..] byte carriers.
    let (vab_carrier, seq_bytes): (std::sync::Arc<Vec<u8>>, Vec<u8>) = if bgm_id >= 2000 {
        // Global pool: one music_01 bank entry carries the whole pair.
        let entry = legaia_engine_core::music_labels::prot_entry_for_bgm_id(bgm_id)?;
        let bytes = index.entry_bytes(entry).ok()?;
        let vab_off = bytes.windows(4).position(|w| w == b"pBAV")?;
        let seq_off = vab_off + bytes[vab_off..].windows(4).position(|w| w == b"pQES")?;
        let seq = bytes[seq_off..].to_vec();
        (bytes, seq)
    } else {
        // Scene-local: SEQ from the id-mapped stream entry, VAB from the
        // scene's first bank entry (the same pair `SceneHost` stages).
        let assets = SceneAssets::build(scene);
        let seq_entry = assets.bgm_seq_entry(bgm_id)?;
        let seq_all = index.entry_bytes(seq_entry).ok()?;
        let off = assets.bgm_seq_offset(bgm_id).unwrap_or(0);
        let seq = seq_all.get(off..)?.to_vec();
        let vab_entry = *assets.vab_entries.first()?;
        (index.entry_bytes(vab_entry).ok()?, seq)
    };
    let vab_off = vab_carrier.windows(4).position(|w| w == b"pBAV")?;
    let report = legaia_vab::parse(&vab_carrier, vab_off).ok()?;
    let seq = legaia_seq::Seq::parse(&seq_bytes).ok()?;

    let mut spu = legaia_engine_audio::Spu::new();
    let mut alloc = legaia_engine_audio::spu::ram::SpuAllocator::new(
        0x1000,
        legaia_engine_audio::spu::ram::SPU_RAM_BYTES as u32 - 0x1000,
    );
    let bank = legaia_engine_audio::VabBank::upload(
        &mut spu,
        &mut alloc,
        &report,
        &vab_carrier[vab_off..],
    );
    let mut sequencer = legaia_engine_audio::sequencer::Sequencer::new(seq, bank);
    // End-of-track fallback loop, as the site's pre-render path sets - an
    // in-stream loop marker still wins.
    sequencer.set_loop_to(0);
    const MAX_SECONDS: usize = 300;
    let render = render_bgm_loop_region(
        &mut sequencer,
        &mut spu,
        MAX_SECONDS * SPU_INTERNAL_RATE as usize,
    );
    // Keep only the repeatable body: hard-looping [loop_start, loop_end) is
    // seamless; keeping the lead-in would seam every repeat.
    let looped = render.loop_start_sample > 0 || render.loop_end_sample > 0;
    let pcm = if render.loop_start_sample < render.loop_end_sample {
        &render.pcm[render.loop_start_sample * 2..render.loop_end_sample * 2]
    } else {
        &render.pcm[..]
    };
    if pcm.is_empty() {
        return None;
    }
    let music_dir = dir.join("music");
    std::fs::create_dir_all(&music_dir).ok()?;
    let file = format!("music/bgm_{bgm_id}.wav");
    write_wav(&music_dir.join(format!("bgm_{bgm_id}.wav")), pcm).ok()?;
    let seconds = (pcm.len() / 2) as f32 / SPU_INTERNAL_RATE as f32;
    println!(
        "  [music] bgm {bgm_id}{}: {seconds:.1}s loop -> {file}",
        legaia_engine_core::music_labels::label_for_bgm_id(bgm_id)
            .map(|t| format!(" ({t})"))
            .unwrap_or_default()
    );
    Some(json!({
        "file": file,
        "bgm_id": bgm_id,
        "title": legaia_engine_core::music_labels::label_for_bgm_id(bgm_id),
        "sample_rate": SPU_INTERNAL_RATE,
        "seamless_loop": looped,
        "loop_seconds": seconds,
    }))
}

/// Export every equipment item of the four player battle files as animated
/// `.glb`s (see `glb_export::export_equipment_item_glbs`). `SCUS_942.54`
/// supplies item names + section labels when readable; the export still
/// works without it (ids in place of names).
fn export_items(
    index: &ProtIndex,
    out: &Path,
    extracted_root: &Path,
    disc: Option<&Path>,
) -> Result<()> {
    use legaia_engine_core::Vfs;
    let scus: Option<Vec<u8>> = match disc {
        Some(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()
            .and_then(|v| v.read("SCUS_942.54").ok()),
        None => legaia_engine_core::DirVfs::new(extracted_root)
            .ok()
            .and_then(|v| v.read("SCUS_942.54").ok()),
    };
    if scus.is_none() {
        eprintln!("  [items] SCUS_942.54 not readable - exporting with ids instead of names");
    }
    let export = export_equipment_item_glbs(index, scus.as_deref())
        .map_err(|e| anyhow::anyhow!("equipment item export: {e}"))?;
    let items_dir = out.join("items");
    let mut written = 0usize;
    for it in &export.items {
        for (bytes, suffix) in [(&it.glb_alone, "_alone"), (&it.glb_with_limb, "_with_limb")] {
            if bytes.is_empty() {
                continue;
            }
            let path = items_dir.join(format!("{}{suffix}.glb", it.file_stem));
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&path, bytes)?;
            written += 1;
        }
    }
    std::fs::write(
        items_dir.join("manifest.json"),
        serde_json::to_string_pretty(&items_manifest(&export))?,
    )?;
    for n in &export.notes {
        eprintln!("  [items] {n}");
    }
    println!(
        "export-glb --items: {} item record(s), {written} glb file(s) -> {}",
        export.items.len(),
        items_dir.display()
    );
    Ok(())
}
