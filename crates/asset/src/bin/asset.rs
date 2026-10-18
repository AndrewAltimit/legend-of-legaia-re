// The slot-art manifest's `serde_json::json!` tree is deep enough to clear
// the default macro recursion limit.
#![recursion_limit = "256"]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[path = "asset/actors.rs"]
mod actors;
#[path = "asset/catalogs.rs"]
mod catalogs;
#[path = "asset/cli_args.rs"]
mod cli_args;
#[path = "asset/cli_io.rs"]
mod cli_io;
#[path = "asset/disc_map.rs"]
mod disc_map;
#[path = "asset/dispatch.rs"]
mod dispatch;
#[path = "asset/field_census.rs"]
mod field_census;
#[path = "asset/field_disasm_cli.rs"]
mod field_disasm_cli;
#[path = "asset/minigame.rs"]
mod minigame;
#[path = "asset/overlay.rs"]
mod overlay;
#[path = "asset/packs.rs"]
mod packs;
#[path = "asset/shops.rs"]
mod shops;
#[path = "asset/side_effect.rs"]
mod side_effect;
#[path = "asset/stage.rs"]
mod stage;
#[path = "asset/summon.rs"]
mod summon;
#[path = "asset/tables.rs"]
mod tables;
#[path = "asset/validation.rs"]
mod validation;
#[path = "asset/worldmap.rs"]
mod worldmap;

use actors::*;
use catalogs::*;
use cli_args::*;
use dispatch::*;
use field_census::*;
use minigame::*;
use overlay::*;
use packs::*;
use shops::*;
use side_effect::*;
use stage::*;
use summon::*;
use tables::*;
use validation::*;
use worldmap::*;

/// Restore default SIGPIPE behaviour so piping into `head` etc. exits
/// quietly instead of panicking on a broken-pipe write.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Resolve `input` to an extracted PROT entry: a path if it exists, else a
/// bare extraction index looked up under `prot_dir`.
fn resolve_prot_entry(input: &str, prot_dir: &std::path::Path) -> Result<PathBuf> {
    let direct = PathBuf::from(input);
    if direct.is_file() {
        return Ok(direct);
    }
    let idx: u32 = input
        .parse()
        .map_err(|_| anyhow::anyhow!("{input} is neither a file nor an extraction index"))?;
    let prefix = format!("{idx:04}_");
    for ent in
        std::fs::read_dir(prot_dir).with_context(|| format!("reading {}", prot_dir.display()))?
    {
        let path = ent?.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(&prefix))
        {
            return Ok(path);
        }
    }
    anyhow::bail!("no entry {prefix}*.BIN under {}", prot_dir.display())
}

#[allow(clippy::too_many_arguments)]
fn account_cmd(
    input: &str,
    prot_dir: &std::path::Path,
    prot_index: Option<u32>,
    funcs: Option<PathBuf>,
    depth: u8,
    rescan: bool,
    min_residue: usize,
    keep_claims: bool,
    json: bool,
) -> Result<()> {
    use legaia_disc_audit::byte_account::{
        AccountOptions, account, prot_index_from_name, render_text,
    };

    let path = resolve_prot_entry(input, prot_dir)?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("<entry>")
        .to_string();
    let opts = AccountOptions {
        prot_index: prot_index.or_else(|| prot_index_from_name(&name)),
        label: name,
        funcs_dir: funcs,
        // The overlay walkers need the sibling entries to measure this image's
        // inherited tail (`legaia_disc_audit::inherited_tail`); this is the same
        // directory the bare-index form resolves against.
        prot_dir: prot_dir.is_dir().then(|| prot_dir.to_path_buf()),
        depth,
        rescan,
        min_residue,
        keep_claims,
        max_nested: if json { 4 } else { 8 },
    };
    let acc = account(&bytes, &opts);
    if json {
        println!("{}", serde_json::to_string_pretty(&acc)?);
    } else {
        print!("{}", render_text(&acc, 0));
    }
    Ok(())
}

fn main() -> Result<()> {
    reset_sigpipe();
    match Cli::parse().cmd {
        Cmd::Describe { input, count } => describe(&input, count),
        Cmd::DiscMap { disc, out } => disc_map::disc_map_cmd(&disc, out.as_deref()),
        Cmd::Account {
            input,
            prot_dir,
            prot_index,
            funcs,
            depth,
            no_rescan,
            min_residue,
            claims,
            json,
        } => account_cmd(
            &input,
            &prot_dir,
            prot_index,
            funcs,
            depth,
            !no_rescan,
            min_residue,
            claims,
            json,
        ),
        Cmd::Decode {
            input,
            type_size,
            offset,
            mode,
            out,
        } => decode_one(&input, type_size, offset, mode, out.as_ref()),
        Cmd::Scan { dir, count } => scan(&dir, count),
        Cmd::Stream { input, max_chunks } => stream_one(&input, max_chunks),
        Cmd::ScanStream {
            dir,
            max_chunks,
            only_hits,
            min_chunks,
        } => scan_stream(&dir, max_chunks, only_hits, min_chunks),
        Cmd::Extract {
            input,
            out,
            save_trailer,
        } => extract_streaming(&input, &out, save_trailer),
        Cmd::TimScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tim_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::TimCatalog { prot, out, rollup } => tim_catalog_cmd(&prot, out.as_deref(), rollup),
        Cmd::TimDeepCatalog { prot, out, rollup } => {
            tim_deep_catalog_cmd(&prot, out.as_deref(), rollup)
        }
        Cmd::BattleTextureCatalog {
            prot,
            scus,
            out,
            rollup,
        } => battle_texture_catalog_cmd(&prot, scus.as_deref(), out.as_deref(), rollup),
        Cmd::TimRenderDistinct { prot, out, tier } => tim_render_distinct_cmd(&prot, &out, tier),
        Cmd::TmdScan {
            dir,
            cdname,
            only_hits,
            out,
        } => tmd_scan_cmd(&dir, cdname.as_deref(), only_hits, out.as_deref()),
        Cmd::ClutFinder {
            extracted_root,
            x,
            y,
            clut_only,
        } => clut_finder_cmd(&extracted_root, x, y, clut_only),
        Cmd::Stage {
            input,
            head,
            verts,
            obj_out,
        } => stage_one(&input, head, verts, obj_out.as_deref()),
        Cmd::StageScan {
            dir,
            cdname,
            only_hits,
        } => stage_scan_cmd(&dir, cdname.as_deref(), only_hits),
        Cmd::FieldPack {
            input,
            all_slots,
            groups,
        } => field_pack_one(&input, all_slots, groups),
        Cmd::FieldPackScan { dir, only_hits } => field_pack_scan(&dir, only_hits),
        Cmd::EffectBundle { input, all_slots } => effect_bundle_one(&input, all_slots),
        Cmd::BattleDataPack { input, out } => battle_data_pack_one(&input, out.as_deref()),
        Cmd::BattleDataPackScan {
            dir,
            cdname,
            only_hits,
        } => battle_data_pack_scan(&dir, cdname.as_deref(), only_hits),
        Cmd::EffectBundleScan { dir, only_hits } => effect_bundle_scan(&dir, only_hits),
        Cmd::PlayerAnm {
            input,
            desc_count,
            out,
        } => player_anm_one(&input, desc_count, out.as_deref()),
        Cmd::PlayerAnmScan {
            dir,
            cdname,
            desc_count,
        } => player_anm_scan(&dir, cdname.as_deref(), desc_count),
        Cmd::SceneV12 {
            input,
            scripts,
            max_scripts,
        } => scene_v12_one(&input, scripts, max_scripts),
        Cmd::SceneV12Scan {
            dir,
            cdname,
            only_hits,
        } => scene_v12_scan(&dir, cdname.as_deref(), only_hits),
        Cmd::WorldmapMenu { scus, json } => worldmap_menu_cmd(&scus, json),
        Cmd::ItemTables {
            scus,
            equipment_only,
            consumables_only,
        } => item_tables_cmd(&scus, equipment_only, consumables_only),
        Cmd::ShopStock {
            prot,
            scus,
            cdname,
            scene,
            entry,
            json,
        } => shop_stock_cmd(
            &prot,
            &scus,
            cdname.as_deref(),
            scene.as_deref(),
            entry,
            json,
        ),
        Cmd::SpellNames { scus, json } => spell_names_cmd(&scus, json),
        Cmd::StealTable { scus, all, json } => steal_table_cmd(&scus, all, json),
        Cmd::AccessoryPassive { scus, json } => accessory_passive_cmd(&scus, json),
        Cmd::SfxTable { scus, json } => sfx_table_cmd(&scus, json),
        Cmd::NewGame { scus, json } => new_game_cmd(&scus, json),
        Cmd::LevelUp { scus, json } => level_up_cmd(&scus, json),
        Cmd::KingdomSlot {
            input,
            slot,
            out,
            wireframe_obj,
        } => kingdom_slot_cmd(&input, slot, out.as_deref(), wireframe_obj.as_deref()),
        Cmd::Slot4Png {
            input,
            from_raw,
            out,
            placements,
            kingdom,
            width,
            height,
            margin,
            only_body,
            frame_body,
            close_polylines,
            style,
            axes,
        } => slot4_png_cmd(
            input.as_deref(),
            from_raw.as_deref(),
            &out,
            placements.as_deref(),
            &kingdom,
            width,
            height,
            margin,
            only_body,
            frame_body,
            close_polylines,
            &style,
            &axes,
        ),
        Cmd::BootOverlay { prot_dir } => boot_overlay_cmd(prot_dir.as_deref()),
        Cmd::SlotScene {
            overlay,
            art,
            frames,
        } => slot_scene_cmd(&overlay, art.as_deref(), frames),
        Cmd::SlotArt { overlay, art, out } => slot_art_cmd(&overlay, &art, &out),
        Cmd::Man {
            input,
            with_encounter,
            max_formations,
            max_regions,
        } => man_one(&input, with_encounter, max_formations, max_regions),
        Cmd::ManScan { dir, cdname, json } => man_scan(&dir, cdname.as_deref(), json),
        Cmd::FieldDisasm { cmd } => field_disasm_cli::field_disasm(cmd),
        Cmd::FieldOpCensus {
            dir,
            cdname,
            csv,
            only,
            context,
        } => field_op_census(
            &dir,
            cdname.as_deref(),
            csv.as_deref(),
            only.as_deref(),
            context,
        ),
        Cmd::MonsterArchive {
            input,
            id,
            obj,
            texture_png,
            palette,
            anim,
            glb,
            dump_block,
            write_block,
        } => monster_archive_one(
            &input,
            id,
            obj.as_deref(),
            texture_png.as_deref(),
            palette,
            anim,
            glb.as_deref(),
            dump_block.as_deref(),
            write_block.as_deref(),
        ),
        Cmd::CharacterPack {
            input,
            slot,
            equip,
            out,
        } => character_pack_one(&input, slot, equip, out.as_deref()),
        Cmd::BattleCharPack {
            input,
            atlas_entry,
            slot,
            out_tmd,
            atlas,
            out_tim,
        } => battle_char_pack_one(
            &input,
            atlas_entry.as_deref(),
            slot,
            out_tmd.as_deref(),
            atlas,
            out_tim.as_deref(),
        ),
        Cmd::FieldCharTex {
            input,
            entry,
            out_tim,
        } => field_char_tex_one(&input, entry, out_tim.as_deref()),
        Cmd::Validate {
            dir,
            cdname,
            counts,
            only_hits,
        } => validate_blocks(&dir, cdname.as_ref(), &counts, only_hits),
        Cmd::BefectCluster {
            prot,
            cdname,
            out,
            json,
        } => befect_cluster_cmd(&prot, &cdname, out.as_deref(), json),
        Cmd::Categorize {
            dir,
            out,
            top_signatures,
            examples,
            filter_class,
            cdname,
        } => categorize_dir(
            &dir,
            out.as_ref(),
            top_signatures,
            examples,
            filter_class.as_deref(),
            cdname.as_deref(),
        ),
        Cmd::FindOverlay {
            dir,
            top,
            lzs_sizes,
        } => find_overlay(&dir, top, &lzs_sizes),
        Cmd::SummonOverlay { input, base, trim } => summon_overlay_cmd(&input, base, trim),
        Cmd::SummonReadef {
            input,
            texture_png_dir,
            clut_sub,
            action_id,
        } => summon_readef_cmd(&input, texture_png_dir.as_deref(), clut_sub, action_id),
        Cmd::MovePower {
            input,
            json,
            effect_index,
        } => {
            if effect_index {
                move_power_effect_index_cmd(&input, json)
            } else {
                move_power_cmd(&input, json)
            }
        }
        Cmd::ModeTable { input, json } => mode_table_cmd(&input, json),
        Cmd::ElementAffinity { input, json } => element_affinity_cmd(&input, json),
        Cmd::SeruSideEffect { input, json } => seru_side_effect_cmd(&input, json),
        Cmd::FormationCensus { input, json } => formation_census_cmd(&input, json),
        Cmd::SummonCreatures { scus, json } => summon_creatures_cmd(scus.as_deref(), json),
        Cmd::Overlay { cmd } => match cmd {
            OverlayCmd::List { json } => overlay_list_cmd(json),
            OverlayCmd::Extract {
                prot_dat,
                out,
                label,
            } => overlay_extract_cmd(&prot_dat, &out, label.as_deref()),
            OverlayCmd::Verify { prot_dat } => overlay_verify_cmd(&prot_dat),
            OverlayCmd::Ghidra { out } => overlay_ghidra_cmd(&out),
            OverlayCmd::Generate {
                prot_dat,
                indices,
                min_votes,
            } => overlay_generate_cmd(&prot_dat, &indices, min_votes),
            OverlayCmd::Scan {
                prot_dat,
                from,
                to,
                min_votes,
                base,
                json,
            } => overlay_scan_cmd(&prot_dat, from, to, min_votes, base, json),
            OverlayCmd::FindSig {
                prot_dat,
                sig_hex,
                anchor_va,
                from,
                to,
            } => overlay_find_sig_cmd(&prot_dat, &sig_hex, anchor_va, from, to),
        },
    }
}
