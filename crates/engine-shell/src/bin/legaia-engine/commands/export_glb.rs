//! `export-glb`: bake a scene (or every scene) into the textured `.glb` +
//! manifest set for Unity / VRChat world building. Thin driver over the
//! shared kernels: `engine-core::scene_assembly` (the same assembly the
//! browser field-scene page renders) + `engine-core::glb_export` (the
//! composition + baking). See `docs/tooling/vrchat-world-export.md`.

use super::*;
use legaia_engine_core::glb_export::{
    FloorSampler, GlbExportOptions, export_animated_prop_glbs, export_equipment_item_glbs,
    export_npc_glbs, export_world_glb, items_manifest, world_manifest,
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
    let world = export_world_glb(&scene, &a, opts).map_err(|e| anyhow::anyhow!(e))?;
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
    let manifest = world_manifest(
        &a,
        opts,
        &world,
        &world_file,
        catalog.as_ref(),
        &npcs,
        &props,
        &floor,
    );
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
