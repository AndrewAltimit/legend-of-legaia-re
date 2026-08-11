//! `monster-model` - export a monster's 3D model as editable OBJ+PNG, or
//! replace it with a custom one.
//!
//! Export writes `<stem>.obj` / `<stem>.mtl` / `<stem>.png` via
//! [`legaia_asset::monster_model::export_obj`]. Replace runs the reverse
//! codec, splices the new mesh + texture pool into the monster's decoded
//! block ([`monster_archive::replace_mesh_and_pool`] - every offset the size
//! change moves is fixed up), re-packs the LZS slot, and patches it in place
//! exactly like `monster-block`.
//!
//! The replacement model must keep the retail part count (`o part_NN`
//! groups): battle animations pose parts rigidly by index, so a same-rig
//! model performs every retail move - including streamed specials - with no
//! animation work at all.
//!
//! Size guards: the decoded block may not grow past the retail block's size
//! unless `--allow-grow` is passed (the battle heap budget is tuned to the
//! retail data; see `docs/subsystems/battle.md`), and the re-packed LZS
//! stream must fit the fixed `0x14000` archive slot either way.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_asset::{monster_archive, monster_model};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_patcher::ppf;

use crate::util::{cue_contents, load_image, note_overwrite};

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_monster_model(
    input: &Path,
    id: u16,
    export: Option<&Path>,
    obj: Option<&Path>,
    texture: Option<&Path>,
    allow_grow: bool,
    dry_run: bool,
    output: Option<&Path>,
    patch: Option<&Path>,
) -> Result<()> {
    if export.is_none() && obj.is_none() {
        bail!(
            "pass --export <stem> to extract and/or --obj <model.obj> --texture <page.png> to replace"
        );
    }
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;
    // The extended footprint, because a retail LZS stream may spill past its
    // own slot; the decoder stops at the block's declared size.
    let entry = patcher.read_entry_footprint(MONSTER_ARCHIVE_ENTRY)?;
    let Some(mesh) = monster_archive::mesh(&entry, id)? else {
        bail!("monster id {id}: empty / filler slot (monster-stats lists the populated ids)");
    };
    let name = monster_archive::record(&entry, id)?
        .map(|r| r.name)
        .unwrap_or_else(|| "?".into());
    let retail_tmd = legaia_tmd::parse(mesh.tmd_bytes())
        .with_context(|| format!("id {id} ({name}): retail mesh unparseable"))?;
    let retail_stats = retail_tmd.stats();

    if let Some(stem) = export {
        let stem_name = stem
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "model".into());
        let exported = monster_model::export_obj(&mesh, &stem_name)?;
        let obj_path = stem.with_extension("obj");
        let mtl_path = stem.with_extension("mtl");
        let png_path = stem.with_extension("png");
        std::fs::write(&obj_path, &exported.obj)
            .with_context(|| format!("write {}", obj_path.display()))?;
        std::fs::write(&mtl_path, &exported.mtl)
            .with_context(|| format!("write {}", mtl_path.display()))?;
        legaia_tim::write_png(
            &png_path,
            exported.page_width,
            exported.page_height,
            &exported.rgba,
        )?;
        println!(
            "exported id {id} {name}: {} parts, {} verts, {} prims -> {} / {} / {} ({}x{})",
            retail_tmd.objects.len(),
            retail_stats.total_vertices,
            retail_stats.total_primitives,
            obj_path.display(),
            mtl_path.display(),
            png_path.display(),
            exported.page_width,
            exported.page_height,
        );
        println!(
            "edit in Blender (raw GTE units, y-down; keep all {} `o part_NN` groups; \
             materials skin_pNN / skin_semi_abrN_pNN / flat carry the render state)",
            retail_tmd.objects.len()
        );
    }

    if let Some(obj_path) = obj {
        let Some(texture_path) = texture else {
            bail!("--obj needs --texture <page.png> (the edited texture page)");
        };
        if !dry_run && output.is_none() && patch.is_none() {
            bail!("--obj needs --output <patched.bin> and/or --patch <out.ppf> (or --dry-run)");
        }
        let obj_text = std::fs::read_to_string(obj_path)
            .with_context(|| format!("read {}", obj_path.display()))?;
        let png_bytes = std::fs::read(texture_path)
            .with_context(|| format!("read {}", texture_path.display()))?;
        let (w, h, rgba) = legaia_tim::encode::decode_png_rgba(&png_bytes)?;
        let retail_page_width = mesh
            .texture()
            .map(|t| t.width)
            .ok_or_else(|| anyhow::anyhow!("id {id}: retail texture pool undecodable"))?;
        if (w, h) != (retail_page_width, monster_model::PAGE_HEIGHT) {
            bail!(
                "texture page is {w}x{h}, id {id}'s retail page is {retail_page_width}x{} - \
                 the page dimensions drive the battle loader's VRAM upload and must match",
                monster_model::PAGE_HEIGHT
            );
        }

        let imported = monster_model::import_obj(&obj_text, &rgba, w, retail_tmd.objects.len())?;
        for warning in &imported.warnings {
            println!("warning: {warning}");
        }
        let new_block = monster_archive::replace_mesh_and_pool(
            &mesh.block,
            Some(&imported.tmd),
            Some(&imported.pool),
        )?;

        let new_tmd = legaia_tmd::parse(&imported.tmd).context("imported mesh re-parse")?;
        let new_stats = new_tmd.stats();
        println!(
            "id {id} {name}: mesh {} -> {} bytes ({} -> {} verts, {} -> {} prims), \
             pool {} -> {} bytes, {} palette(s), block {} -> {} bytes",
            retail_stats.total_bytes_consumed,
            new_stats.total_bytes_consumed,
            retail_stats.total_vertices,
            new_stats.total_vertices,
            retail_stats.total_primitives,
            new_stats.total_primitives,
            mesh.block.len() - mesh.texture_pool_offset,
            imported.pool.len(),
            imported.palettes_used,
            mesh.block.len(),
            new_block.len(),
        );
        if new_block.len() > mesh.block.len() && !allow_grow {
            bail!(
                "the new block is {} bytes over the retail size - the battle heap budget \
                 is tuned to retail data, so growth risks the loader's unchecked \
                 allocation. Simplify the model/texture, or pass --allow-grow to \
                 accept the risk",
                new_block.len() - mesh.block.len()
            );
        }
        // encode_slot enforces the hard cap: the LZS stream must fit 0x14000.
        let slot = monster_archive::encode_slot(&new_block)?;
        let used = 4 + slot.iter().rposition(|&b| b != 0).map_or(0, |p| p + 1);
        println!(
            "slot fit: {} / {} bytes compressed ({}% full)",
            used,
            monster_archive::SLOT_STRIDE,
            used * 100 / monster_archive::SLOT_STRIDE
        );
        if dry_run {
            println!("dry run - nothing written");
            return Ok(());
        }

        patcher.patch_monster_slot(id, &slot)?;
        let patched = patcher.into_image();
        if let Some(ppf_path) = patch {
            let runs = ppf::diff_runs(&original, &patched);
            let desc = format!("Legaia monster model replacement (id {id})");
            note_overwrite(ppf_path);
            std::fs::write(ppf_path, ppf::write_ppf3(&desc, &runs))
                .with_context(|| format!("write PPF {}", ppf_path.display()))?;
            println!("wrote {} ({} change runs)", ppf_path.display(), runs.len());
        }
        if let Some(out) = output {
            note_overwrite(out);
            std::fs::write(out, &patched)
                .with_context(|| format!("write patched image {}", out.display()))?;
            let cue = out.with_extension("cue");
            let bin_name = out
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "patched.bin".to_string());
            std::fs::write(&cue, cue_contents(&bin_name))
                .with_context(|| format!("write cue {}", cue.display()))?;
            println!("wrote {} (+ {})", out.display(), cue.display());
        }
        // Verify off the patched image: the slot decodes, the mesh parses,
        // and the animations still address every part.
        let check = DiscPatcher::open(patched).context("re-parse patched image")?;
        let entry = check.read_entry_footprint(MONSTER_ARCHIVE_ENTRY)?;
        let Some(mesh2) = monster_archive::mesh(&entry, id)? else {
            bail!("patched image: id {id}'s block no longer decodes");
        };
        let tmd2 = legaia_tmd::parse(mesh2.tmd_bytes())
            .context("patched image: replacement mesh unparseable")?;
        if tmd2.objects.len() != retail_tmd.objects.len() {
            bail!("patched image: part count changed");
        }
        let anims = monster_archive::animations(&entry, id)?
            .ok_or_else(|| anyhow::anyhow!("patched image: animations no longer decode"))?;
        for a in &anims {
            if a.part_count != tmd2.objects.len() {
                bail!(
                    "patched image: animation poses {} parts, mesh has {}",
                    a.part_count,
                    tmd2.objects.len()
                );
            }
        }
        println!(
            "verified: id {id} {name} decodes off the patched image ({} parts, {} animations intact)",
            tmd2.objects.len(),
            anims.len()
        );
    }
    Ok(())
}
