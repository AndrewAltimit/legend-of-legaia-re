//! Tactical Arts stages: power edits, the AP overrides, the Super Art list and pack, and the combo randomizer.

use anyhow::Result;

use legaia_patcher::apply;
use legaia_patcher::drops::DropMode;

use super::Run;

/// Tactical-Art damage-power edits: rewrite an art's per-strike power bytes
/// (record+0x24) in the character's player-file record0. Seedless targeted
/// edits keyed by input combo.
pub(super) fn arts_power(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if !args.arts_power.is_empty() {
        let report = apply::set_arts_power(patcher, &args.arts_power)?;
        let mut changed: std::collections::BTreeSet<Vec<u8>> = std::collections::BTreeSet::new();
        for e in &report.edits {
            let combo: String = e
                .combo
                .iter()
                .map(legaia_arts_patch::arts_power::command_glyph)
                .collect();
            let old: String = e
                .old_power
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let new: String = e
                .new_power
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!("arts-power: {combo} ({:?}) [{old}] -> [{new}]", e.character);
            changed.insert(e.combo.iter().map(|c| c.as_byte()).collect());
        }
        for (combo, value) in &args.arts_power {
            let key: Vec<u8> = combo.iter().map(|c| c.as_byte()).collect();
            let combo_s: String = combo
                .iter()
                .map(legaia_arts_patch::arts_power::command_glyph)
                .collect();
            if !changed.contains(&key) {
                println!("arts-power: {combo_s} already at {value:#04X} (or has no damage byte)");
            }
            manifest.push(format!("arts_power {combo_s} = {value:#04X}"));
        }
    }
    Ok(())
}

/// Super Art damage-power edits: rewrite a Super Art's per-strike power bytes
/// (record+0x24) in its own record0 art record, addressed by the finisher
/// action constant and validated against the record's own name field. Seedless
/// targeted edits keyed by Super Art name - a Super Art has no input combo and
/// no arts-name-table row, so neither `--arts-power` nor `--arts-ap-*` can
/// reach it.
pub(super) fn super_art_power(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if !args.super_art_power.is_empty() {
        let report = apply::set_super_art_power(patcher, &args.super_art_power)?;
        let mut changed: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
        for e in &report.edits {
            let old: String = e
                .old_power
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            let new: String = e
                .new_power
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!(
                "super-art-power: {} ({:?}) [{old}] -> [{new}]",
                e.name, e.character
            );
            changed.insert(e.finisher);
        }
        for (art, value) in &args.super_art_power {
            if !changed.contains(&art.finisher) {
                println!(
                    "super-art-power: {} already at {value:#04X} (or has no damage byte)",
                    art.name
                );
            }
            manifest.push(format!("super_art_power {} = {value:#04X}", art.name));
        }
    }
    Ok(())
}

/// Arts AP override: three same-size detours into the party arts queue-builder
/// (PROT 0898) + routines and a per-(character, row) config table in
/// verified-dead SCUS regions, so a targeted art either grants AP (clamped at
/// 100) or charges a flat cost instead of retail's computed one. Each entry
/// lands in its own cell, so one character's art never moves another's. The
/// art's menu AP number (a separate byte in the SCUS arts-name table, the
/// pause menu's only source) is rewritten to match.
pub(super) fn arts_ap_override(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if !args.arts_ap_grant.is_empty() || !args.arts_ap_cost.is_empty() {
        let specs: Vec<_> = args
            .arts_ap_grant
            .iter()
            .chain(args.arts_ap_cost.iter())
            .cloned()
            .collect();
        let report = apply::inject_arts_ap_grant(patcher, &specs)?;
        for g in &report.resolved {
            let combo = legaia_arts_patch::arts_ap_grant::combo_str(&g.combo);
            let what = if g.mode.is_grant() {
                format!("grants {} AP", g.mode.amount())
            } else {
                format!("costs {} AP", g.mode.amount())
            };
            println!(
                "arts-ap: {:?} {combo} {:?} (row {}) {what} - menu list now shows {} (was {})",
                g.character, g.name, g.row, g.display_ap, g.previous_display_ap
            );
        }
        for spec in &specs {
            let combo_s = legaia_arts_patch::arts_ap_grant::combo_str(&spec.combo);
            let who = match spec.character {
                Some(c) => format!("{c:?}:"),
                None => String::new(),
            };
            let key = if spec.mode.is_grant() {
                "arts_ap_grant"
            } else {
                "arts_ap_cost"
            };
            manifest.push(format!("{key} {who}{combo_s} = {}", spec.mode.amount()));
        }
    }
    Ok(())
}

/// Oscillating AP costs: per battle, each Tactical Art lands on the cost
/// side (retail) or the grant side (admitted at any AP, adds the AP it would
/// have cost, deals a fraction of its damage). The arts AP override's three
/// sites plus the strike-damage site in PROT 0898, a roll at the SCUS
/// battle-loader setup site, and the routines in the four dead regions.
pub(super) fn oscillating_ap(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if let Some(pct) = args.oscillating_ap {
        let report = apply::inject_oscillating_ap(patcher, pct)?;
        println!(
            "oscillating-ap: every battle re-deals each art onto the cost side (retail) or the \
             grant side (gives its AP back, {}% damage); roll at {:#x}, damage routine at {:#x}, \
             side table at {:#x}",
            report.damage_pct, report.roll_va, report.damage_va, report.bits_va
        );
        manifest.push(format!("oscillating_ap = {pct}"));
    }
    Ok(())
}

/// Show Super Arts: the in-battle Tactical-Arts list gains, sorted in by AP,
/// the Super Arts the character has performed - name, chain AP cost and the
/// arrows the player types per row (two detours into the SCUS list renderer
/// plus their routines and tables in dead space, a replaced list pager in
/// PROT 0898 whose tail records a performed Super Art, and a detour from the
/// Super applier into it).
pub(super) fn show_super_arts(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.show_super_arts {
        let report = apply::inject_super_art_list(patcher)?;
        println!(
            "show-super-arts: {} Super Arts join the in-battle move list and the status \
             screen's Moves page once performed (hooks at {:#x}/{:#x}, fill {:#x}, \
             performed-byte writer {:#x}, menu row routine {:#x})",
            report.rows.len(),
            report.count_va,
            report.id_va,
            report.fill_va,
            report.performed_va,
            report.menu_row_va
        );
        for row in &report.rows {
            println!(
                "  {:?}: {} - {} AP, {} ({})",
                row.character,
                row.name,
                row.ap,
                row.input,
                row.chain.join(" + ")
            );
        }
        manifest.push("show_super_arts = true".to_string());
    } else {
        manifest.push("show_super_arts = false".to_string());
    }
    Ok(())
}

/// Super Arts Pack (by ZetaPhoenix): fifteen extra Super Arts, five per
/// character, from his own 3764-byte block - parked in the DMY.DAT annex and
/// streamed to 0x801FD000 at battle load, reached by fourteen same-size
/// edited words across ten retail sites.
pub(super) fn super_arts_pack(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.super_arts_pack {
        let report = apply::inject_super_arts_pack(patcher)?;
        println!(
            "super-arts-pack (by ZetaPhoenix): {} extra Super Arts, five per character \
             (block at LBA {} x{} sectors -> {:#x}, loader stub {:#x}, {} word edits)",
            report.names.len(),
            report.block_lba,
            report.block_sectors,
            legaia_patcher::super_arts_pack::BLOCK_VA,
            report.stub_va,
            report.edits
        );
        for (i, name) in report.names.iter().enumerate() {
            let who = ["Vahn", "Noa", "Gala"][i / 5];
            println!("  {who}: {name}");
        }
        // The pack ships with the author's arts name-length fix - his own
        // update to the mod - parked directly behind the battle-load stub.
        let fix = apply::inject_arts_name_fix(
            patcher,
            legaia_patcher::super_arts_pack::ARENA_USED_END_VA,
        )?;
        println!(
            "  + the author's name-length fix: Super/Miracle Art banner re-centred for \
             the installed name (routine at {:#x})",
            fix.routine_va
        );
        manifest.push(
            "super_arts_pack = true  # by ZetaPhoenix, incl. his name-length fix".to_string(),
        );
    } else {
        manifest.push("super_arts_pack = false".to_string());
    }
    Ok(())
}

/// Tactical Art button combos (`--arts`).
pub(super) fn arts_combos(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let seed = run.seed;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let arts_mode = args.arts.mode().map(|m| match m {
        DropMode::Shuffle => legaia_arts_patch::arts::ArtsMode::Shuffle,
        DropMode::Random => legaia_arts_patch::arts::ArtsMode::Random,
    });
    if let Some(arts_mode) = arts_mode {
        let (_plan, report) = apply::randomize_arts(patcher, seed, arts_mode)?;
        println!(
            "arts: {} of {} arts re-combo'd ({:?})",
            report.combos_changed, report.arts, arts_mode
        );
        manifest.push(format!(
            "arts = {:?}",
            match arts_mode {
                legaia_arts_patch::arts::ArtsMode::Shuffle => "shuffle",
                legaia_arts_patch::arts::ArtsMode::Random => "random",
            }
        ));
        manifest.push(format!(
            "arts_changed = {}  # of {} regular arts",
            report.combos_changed, report.arts
        ));
    } else {
        println!("arts: untouched");
        manifest.push("arts = \"none\"".to_string());
    }
    Ok(())
}
