//! `legaia-rando` - top-level randomizer / disc patcher CLI.
//!
//! Reads a **user-supplied** retail Legaia disc image, plans a randomization
//! from a seed, and emits a portable PPF 3.0 patch (the redistributable
//! deliverable) plus, optionally, a full patched `.bin` copy for local play.
//! The patched `.bin` contains Sony bytes and must never be shared - the
//! shareable artifacts are the patcher and the seed.
//!
//! ```text
//! legaia-rando randomize --input DISC.bin --seed mysrun --drops shuffle --patch out.ppf
//! legaia-rando drops     --input DISC.bin      # read-only: list current monster drops
//! ```

mod cli;
mod commands;
mod randomize;
mod util;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Cmd};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Drops { input } => commands::cmd_drops(&input),
        Cmd::Chests { input } => commands::cmd_chests(&input),
        Cmd::Steals { input } => commands::cmd_steals(&input),
        Cmd::Arts { input } => commands::cmd_arts(&input),
        Cmd::Doors { input } => commands::cmd_doors(&input),
        Cmd::HouseDoors { input } => commands::cmd_house_doors(&input),
        Cmd::StartingItems { input } => commands::cmd_starting_items(&input),
        Cmd::Shops { input } => commands::cmd_shops(&input),
        Cmd::Casino { input } => commands::cmd_casino(&input),
        Cmd::MonsterStats { input } => commands::cmd_monster_stats(&input),
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
        } => randomize::cmd_verify(&input, &patch, output.as_deref()),
    }
}
