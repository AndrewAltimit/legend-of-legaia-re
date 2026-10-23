//! The `randomize` and `verify` subcommands: plan a full randomization from a
//! seed, diff the patched image into a PPF (+ optional patched `.bin` / `.cue` /
//! manifest), and apply-and-check an existing PPF.
//!
//! [`cmd_randomize`] is the stage list: one call per feature, in the order the
//! edits must land on the disc. That order is part of a seed's meaning - a
//! later stage reads what an earlier one wrote (the stat scale reads the stat
//! shuffle's roster, a named swing cost overrides the specialty permutation,
//! several stages re-pack the same scene MAN) - so a stage is added at its
//! place in the list, never moved. Each stage lives in the submodule of its
//! feature family and appends its own lines to the run manifest.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_disc_patch::disc::DiscPatcher;
use legaia_patcher::apply;

use crate::cli::RandomizeArgs;
use crate::disc_io::{check_usa_disc, load_image, note_overwrite};
use crate::flag_values::{clock_seed, resolve_seed};

mod arts;
mod battle_hooks;
mod delilas;
mod equipment;
mod monsters;
mod output;
mod pool;
mod starting;
mod world;

/// One `randomize` run: the flags, the resolved seed, the image being
/// patched and the manifest lines the stages accumulate.
pub(crate) struct Run<'a> {
    pub(crate) args: &'a RandomizeArgs,
    pub(crate) seed: u64,
    pub(crate) patcher: DiscPatcher,
    /// Options + outcome, for reproducibility.
    pub(crate) manifest: Vec<String>,
    /// The valid item pool the `random` drop / chest / steal modes draw from
    /// (empty when none of them is on).
    pub(crate) pool: Vec<u8>,
    /// Unused-enemy ids the encounter stage may place (empty unless
    /// `--unused-enemies`).
    pub(crate) unused_enemies: &'static [u8],
}

/// Refuse the flag combinations that cannot share a disc, before touching
/// anything.
fn check_flag_conflicts(args: &RandomizeArgs) -> Result<()> {
    // Arts AP-grant, shiny-Seru, and the Delilas Challenge's dome course all
    // inject into the same verified-dead SCUS arena bytes (0x8007AE00). Arts
    // AP-grant conflicts with either of the others as a hard error (it is
    // manual-only, never in a preset). Shiny-Seru vs the Delilas Challenge is
    // resolved softly below (the challenge wins) so the presets that carry
    // both still patch. Refuse the hard conflicts before touching anything.
    let arts_ap = !args.arts_ap_grant.is_empty() || !args.arts_ap_cost.is_empty();
    // Oscillating AP costs claims all four regions (it reuses the arts AP
    // override's sites and adds a roll + a damage routine), so it is a hard
    // conflict with every other arena feature.
    let oscillating = args.oscillating_ap.is_some();
    for (other, flag) in [
        (arts_ap, "--arts-ap-grant / --arts-ap-cost"),
        (args.shiny_seru, "--shiny-seru"),
        (args.delilas_challenge, "--delilas-challenge"),
        (args.show_super_arts, "--show-super-arts"),
        (args.super_arts_pack, "--super-arts-pack"),
    ] {
        if oscillating && other {
            bail!(
                "--oscillating-ap and {flag} both inject into the same verified-dead SCUS \
                 regions and are mutually exclusive; enable only one"
            );
        }
    }
    if arts_ap && args.shiny_seru {
        bail!(
            "--arts-ap-grant / --arts-ap-cost and --shiny-seru both inject into the same \
             verified-dead SCUS regions and are mutually exclusive; enable only one"
        );
    }
    if arts_ap && args.delilas_challenge {
        bail!(
            "--arts-ap-grant / --arts-ap-cost and --delilas-challenge both inject into the same \
             verified-dead SCUS regions and are mutually exclusive; enable only one"
        );
    }
    // --show-super-arts spans all four verified-dead SCUS regions (its shared
    // unlock leaf and three hook routines, plus the chain / AP / finisher
    // tables and the scratch record), so it is a hard conflict with all three
    // of the arena features - including the Delilas Challenge, which is never
    // silently dropped in its favour.
    for (other, flag) in [
        (arts_ap, "--arts-ap-grant / --arts-ap-cost"),
        (args.shiny_seru, "--shiny-seru"),
        (args.delilas_challenge, "--delilas-challenge"),
    ] {
        if args.show_super_arts && other {
            bail!(
                "--show-super-arts and {flag} both inject into the same verified-dead SCUS \
                 regions and are mutually exclusive; enable only one"
            );
        }
    }
    // The Super Arts Pack hosts its battle-load stub in the
    // same arena, and rewrites the Super-Art applier the move list detours - so
    // it is a hard conflict with every one of them.
    for (other, flag) in [
        (arts_ap, "--arts-ap-grant / --arts-ap-cost"),
        (args.shiny_seru, "--shiny-seru"),
        (args.delilas_challenge, "--delilas-challenge"),
        (args.show_super_arts, "--show-super-arts"),
    ] {
        if args.super_arts_pack && other {
            bail!(
                "--super-arts-pack and {flag} both inject into the same verified-dead SCUS \
                 regions and are mutually exclusive; enable only one"
            );
        }
    }
    // The Delilas Challenge detours a word of the AI picker's Delilas arm at
    // 0x801EB7C0.. and the party swap's enemy-side conversion rewrites that
    // same arm, refusing any encoding but retail or its own. Applied together
    // the swap stops part-way through, so the pair is refused up front.
    if args.delilas_challenge && args.delilas_party.is_some() {
        bail!(
            "--delilas-challenge and --delilas-party both rewrite the Delilas arm of the \
             battle AI picker and are mutually exclusive; enable only one"
        );
    }
    Ok(())
}

pub(crate) fn cmd_randomize(args: RandomizeArgs) -> Result<()> {
    let seed = match args.seed.as_deref() {
        Some(s) if s.trim().is_empty() => {
            bail!("empty --seed; pass a number or a non-empty string (or omit for a clock seed)")
        }
        Some(s) => resolve_seed(s),
        None => clock_seed(),
    };
    check_flag_conflicts(&args)?;
    let original = load_image(&args.input)?;
    check_usa_disc(&original, args.allow_region_mismatch, "randomize")?;
    let patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    println!("seed: {seed} (0x{seed:016X})");
    // Manifest lines accumulate the run's options + outcome for reproducibility.
    let manifest = vec![
        "# legaia-patcher run manifest".to_string(),
        format!("seed = {seed}  # 0x{seed:016X}"),
        format!("input = {:?}", args.input.display().to_string()),
    ];
    let mut run = Run {
        args: &args,
        seed,
        patcher,
        manifest,
        pool: Vec::new(),
        unused_enemies: &[],
    };

    let (pool, unused_enemies) = pool::item_pool(&mut run)?;
    run.pool = pool;
    run.unused_enemies = unused_enemies;

    monsters::drops(&mut run)?;
    battle_hooks::equipment_drops(&mut run)?;
    battle_hooks::flee_exp(&mut run)?;
    battle_hooks::enemy_ally(&mut run)?;
    battle_hooks::enemy_hp_bar(&mut run)?;
    battle_hooks::shiny_seru(&mut run)?;
    delilas::delilas_challenge(&mut run)?;
    delilas::delilas_party(&mut run)?;
    battle_hooks::jewel_fix(&mut run)?;
    battle_hooks::approach_softlock_fix(&mut run)?;
    world::fortress_encounter_fix(&mut run)?;
    world::conkram_freeze_fix(&mut run)?;
    battle_hooks::spirit_ap(&mut run)?;
    battle_hooks::damage_ap(&mut run)?;
    world::fishing_price(&mut run)?;
    world::earth_egg_price(&mut run)?;
    arts::arts_power(&mut run)?;
    arts::super_art_power(&mut run)?;
    arts::arts_ap_override(&mut run)?;
    arts::oscillating_ap(&mut run)?;
    arts::show_super_arts(&mut run)?;
    arts::super_arts_pack(&mut run)?;
    world::rename_location(&mut run)?;
    world::seru_trade(&mut run)?;
    world::encounters(&mut run)?;
    world::chests(&mut run)?;
    world::shops(&mut run)?;
    world::casino(&mut run)?;
    monsters::monster_stats(&mut run)?;
    monsters::enemy_stat_scale(&mut run)?;
    monsters::exp_scale(&mut run)?;
    monsters::seru_catch_rate(&mut run)?;
    monsters::enemy_attack_count(&mut run)?;
    monsters::move_power(&mut run)?;
    monsters::element_affinity(&mut run)?;
    monsters::spell_cost(&mut run)?;
    equipment::weapon_specialty(&mut run)?;
    let relayout_sectors = equipment::equipment_edits(&mut run)?;
    equipment::equip_bonus(&mut run)?;
    equipment::equip_mask(&mut run)?;
    monsters::steals(&mut run)?;
    arts::arts_combos(&mut run)?;
    world::doors(&mut run)?;
    world::house_doors(&mut run)?;
    starting::starting_items(&mut run)?;
    starting::starting_level(&mut run)?;

    // Diff original vs patched -> PPF.
    let Run {
        patcher, manifest, ..
    } = run;
    let patched = patcher.into_image();
    output::write_outputs(&args, seed, &original, &patched, manifest, relayout_sectors)
}

/// Apply a PPF to a copy of the disc and confirm the result still parses.
pub(crate) fn cmd_verify(
    input: &Path,
    patch: &Path,
    output: Option<&Path>,
    allow_region_mismatch: bool,
) -> Result<()> {
    let mut image = load_image(input)?;
    let disc_label = check_usa_disc(&image, allow_region_mismatch, "this patch")?;
    println!("disc: {disc_label}");
    let ppf = std::fs::read(patch).with_context(|| format!("read patch {}", patch.display()))?;
    let applied =
        legaia_disc_patch::ppf::apply_ppf3(&mut image, &ppf).context("apply PPF to disc image")?;
    // Re-parse the patched image end to end as a sanity check.
    let patcher = DiscPatcher::open(image).context("patched image no longer parses as a disc")?;
    let drops = apply::current_drops(&patcher)
        .map(|d| d.iter().filter(|x| x.item != 0).count())
        .unwrap_or(0);
    println!(
        "verify OK: {applied} PPF records applied; disc parses ({} PROT entries, {drops} monster drops)",
        patcher.entry_count()
    );
    if let Some(out) = output {
        note_overwrite(out);
        std::fs::write(out, patcher.image()).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes - do not redistribute)",
            out.display()
        );
    }
    Ok(())
}
