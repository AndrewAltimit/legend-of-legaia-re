//! `legaia-patcher` - top-level randomizer / disc patcher CLI.
//!
//! Reads a **user-supplied** retail Legaia disc image, plans a randomization
//! from a seed, and emits a portable PPF 3.0 patch (the redistributable
//! deliverable) plus, optionally, a full patched `.bin` copy for local play.
//! The patched `.bin` contains Sony bytes and must never be shared - the
//! shareable artifacts are the patcher and the seed.
//!
//! ```text
//! legaia-patcher randomize --input DISC.bin --seed mysrun --drops shuffle --patch out.ppf
//! legaia-patcher drops     --input DISC.bin      # read-only: list current monster drops
//! ```

mod cli;
mod commands;
mod monster_block;
mod monster_model;
mod randomize;
mod save_icon;
mod texture;
mod translate;
mod util;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Cmd};

/// Restore the default SIGPIPE disposition so piping into `head` etc.
/// terminates the process quietly instead of panicking on a broken pipe.
fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

fn main() -> Result<()> {
    reset_sigpipe();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Drops { input } => commands::cmd_drops(&input),
        Cmd::Chests { input } => commands::cmd_chests(&input),
        Cmd::Steals { input } => commands::cmd_steals(&input),
        Cmd::Arts { input } => commands::cmd_arts(&input),
        Cmd::Doors { input } => commands::cmd_doors(&input),
        Cmd::HouseDoors { input } => commands::cmd_house_doors(&input),
        Cmd::MapDoors { input } => commands::cmd_map_doors(&input),
        Cmd::StartingItems { input } => commands::cmd_starting_items(&input),
        Cmd::Shops { input } => commands::cmd_shops(&input),
        Cmd::Casino { input } => commands::cmd_casino(&input),
        Cmd::Fishing { input } => commands::cmd_fishing(&input),
        Cmd::DelilasPokes { custom_items } => commands::cmd_delilas_pokes(custom_items),
        Cmd::EarthEgg { input } => commands::cmd_earth_egg(&input),
        Cmd::Locations { input } => commands::cmd_locations(&input),
        Cmd::MonsterStats { input } => commands::cmd_monster_stats(&input),
        Cmd::MonsterBlock {
            input,
            id,
            dump,
            write,
            output,
            patch,
        } => monster_block::cmd_monster_block(
            &input,
            id,
            dump.as_deref(),
            write.as_deref(),
            output.as_deref(),
            patch.as_deref(),
        ),
        Cmd::MonsterModel {
            input,
            id,
            export,
            obj,
            texture,
            allow_grow,
            dry_run,
            output,
            patch,
        } => monster_model::cmd_monster_model(
            &input,
            id,
            export.as_deref(),
            obj.as_deref(),
            texture.as_deref(),
            allow_grow,
            dry_run,
            output.as_deref(),
            patch.as_deref(),
        ),
        Cmd::TimList { input, entry, tier } => texture::cmd_tim_list(&input, entry, tier),
        Cmd::TimExport {
            input,
            entry,
            offset,
            lzs_section,
            battle_slot,
            clut,
            output,
        } => texture::cmd_tim_export(
            &input,
            entry,
            offset,
            lzs_section,
            battle_slot,
            clut,
            &output,
        ),
        Cmd::TimReplace {
            input,
            entry,
            offset,
            lzs_section,
            battle_slot,
            clut,
            png,
            quantize,
            output,
            patch,
            dry_run,
        } => texture::cmd_tim_replace(
            &input,
            entry,
            offset,
            lzs_section,
            battle_slot,
            clut,
            &png,
            quantize,
            output.as_deref(),
            patch.as_deref(),
            dry_run,
        ),
        Cmd::SaveIconList { input } => save_icon::cmd_save_icon_list(&input),
        Cmd::SaveIconExport {
            input,
            slot,
            output,
        } => save_icon::cmd_save_icon_export(&input, slot, &output),
        Cmd::SaveIconReplace {
            input,
            slot,
            png,
            quantize,
            output,
            patch,
            dry_run,
        } => save_icon::cmd_save_icon_replace(
            &input,
            slot,
            &png,
            quantize,
            output.as_deref(),
            patch.as_deref(),
            dry_run,
        ),
        Cmd::MovePowers { input } => commands::cmd_move_powers(&input),
        Cmd::Affinity { input } => commands::cmd_affinity(&input),
        Cmd::SpellCosts { input } => commands::cmd_spell_costs(&input),
        Cmd::EquipBonuses { input } => commands::cmd_equip_bonuses(&input),
        Cmd::WeaponSpecialty { input } => commands::cmd_weapon_specialty(&input),
        Cmd::Randomize(args) => randomize::cmd_randomize(args),
        Cmd::Verify {
            input,
            patch,
            output,
            allow_region_mismatch,
        } => randomize::cmd_verify(&input, &patch, output.as_deref(), allow_region_mismatch),
        Cmd::Translate { cmd } => match cmd {
            cli::TranslateCmd::Export { input, output } => translate::cmd_export(&input, &output),
            cli::TranslateCmd::Init {
                lang,
                from,
                input,
                contributor,
                resume,
                chunk,
                output,
            } => translate::cmd_init(
                &lang,
                from.as_deref(),
                input.as_deref(),
                contributor,
                resume.as_deref(),
                chunk,
                &output,
            ),
            cli::TranslateCmd::Strip {
                pack,
                output,
                notes,
            } => translate::cmd_strip(&pack, &output, notes.as_deref()),
            cli::TranslateCmd::Merge {
                base,
                packs,
                output,
            } => translate::cmd_merge(&base, &packs, &output),
            cli::TranslateCmd::Stats {
                pack,
                input,
                verbose,
            } => translate::cmd_stats(&pack, input.as_deref(), verbose),
            cli::TranslateCmd::Import {
                input,
                pack,
                output,
                patch,
                allow_relayout,
                verbose,
            } => translate::cmd_import(
                &input,
                &pack,
                output.as_deref(),
                patch.as_deref(),
                allow_relayout,
                verbose,
            ),
            cli::TranslateCmd::LiftOfficial {
                from,
                target,
                output,
                fold_accents,
            } => translate::cmd_lift_official(&from, &target, &output, fold_accents),
            cli::TranslateCmd::FitReport { from, target } => {
                translate::cmd_fit_report(&from, &target)
            }
            cli::TranslateCmd::DiffDisc { input, other } => {
                translate::cmd_diff_disc(&input, &other)
            }
        },
    }
}
