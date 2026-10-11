//! Tactical Arts: combos, power, AP overrides, oscillating AP, the Super Arts list and pack.

use super::*;

/// Show Super Arts: the in-battle Tactical-Arts list gains, sorted in by AP,
/// the Super Arts the character has performed - name, chain AP cost and the
/// arrows the player types per row (two detours into the SCUS list renderer
/// plus their routines and tables in dead space, a replaced list pager in
/// PROT 0898 whose tail records a performed Super Art, and a detour from the
/// Super applier into it). Spoiler-safe: the count, never the roster.
pub(super) fn show_super_arts(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        show_super_arts, ..
    } = *run.opts;
    if show_super_arts {
        let rep = apply::inject_super_art_list(&mut run.patcher)
            .map_err(|e| format!("show-super-arts: {e}"))?;
        run.summary.push_str(&format!(
            "show-super-arts: {} Super Arts join the in-battle move list and the status \
             screen's Moves page once performed\n",
            rep.rows.len()
        ));
    } else {
        run.summary.push_str("show-super-arts: untouched\n");
    }
    Ok(())
}

/// Super Arts Pack (by ZetaPhoenix): his 3764-byte block parked in the annex
/// and streamed to 0x801FD000 at battle load, reached by ten word edits.
/// Spoiler-safe: the count, never the roster.
pub(super) fn super_arts_pack(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        super_arts_pack, ..
    } = *run.opts;
    if super_arts_pack {
        let rep = apply::inject_super_arts_pack(&mut run.patcher)
            .map_err(|e| format!("super-arts-pack: {e}"))?;
        // The pack ships with the author's arts name-length fix - his own
        // update to the mod - parked directly behind the battle-load stub.
        apply::inject_arts_name_fix(
            &mut run.patcher,
            legaia_patcher::super_arts_pack::ARENA_USED_END_VA,
        )
        .map_err(|e| format!("super-arts-pack (name-length fix): {e}"))?;
        run.summary.push_str(&format!(
            "super-arts-pack (by ZetaPhoenix): {} extra Super Arts, five per character, \
             incl. the author's name-banner fix\n",
            rep.names.len()
        ));
    } else {
        run.summary.push_str("super-arts-pack: untouched\n");
    }
    Ok(())
}

/// Oscillating AP costs: `DAMAGE_PCT` (0..=100, empty = off). Per battle,
/// each Tactical Art lands on the cost side (retail) or the grant side
/// (admitted at any AP, adds the AP it would have cost, deals DAMAGE_PCT
/// percent of its damage). Mutually exclusive with every other arena
/// feature (guarded above).
pub(super) fn oscillating_ap(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { oscillating_ap, .. } = *run.opts;
    let oscillating_ap = oscillating_ap.trim();
    if oscillating_ap.is_empty() {
        run.summary.push_str("oscillating-ap: off\n");
    } else {
        match oscillating_ap.parse::<u8>() {
            Ok(pct) if pct <= legaia_arts_patch::oscillating_ap::MAX_DAMAGE_PCT => {
                match apply::inject_oscillating_ap(&mut run.patcher, pct) {
                    Ok(rep) => run.summary.push_str(&format!(
                        "oscillating-ap: every battle re-deals each art onto the cost side \
                         (retail) or the grant side (gives its AP back, {}% damage)\n",
                        rep.damage_pct
                    )),
                    Err(e) => run.summary.push_str(&format!("oscillating-ap: {e}\n")),
                }
            }
            _ => run.summary.push_str(&format!(
                "oscillating-ap: skipped out-of-range value {oscillating_ap:?} (want 0..=100)\n"
            )),
        }
    }
    Ok(())
}

/// Arts damage-power edits: comma/space/newline-separated `COMBO=VALUE`
/// tokens (`RDLDL=0x16`). `VALUE` is a power-encoding byte (`0` disables, or
/// `0x0C..=0x1F` = a damage tier; lower = weaker). A bad entry is reported
/// and skipped.
pub(super) fn arts_power(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { arts_powers, .. } = *run.opts;
    let arts_powers = arts_powers.trim();
    if arts_powers.is_empty() {
        run.summary.push_str("arts-power: untouched\n");
    } else {
        for tok in arts_powers
            .split([',', ';', '\n', ' '])
            .filter(|t| !t.trim().is_empty())
        {
            let parsed = tok.split_once('=').and_then(|(c, v)| {
                let combo = legaia_arts_patch::arts_power::parse_combo(c.trim())?;
                let vs = v.trim();
                let value = vs
                    .strip_prefix("0x")
                    .or_else(|| vs.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| vs.parse::<u8>())
                    .ok()?;
                (value == 0 || legaia_arts_patch::arts_power::is_power_byte(value))
                    .then_some((combo, value))
            });
            match parsed {
                Some((combo, value)) => {
                    match apply::set_arts_power(&mut run.patcher, &[(combo, value)]) {
                        Ok(rep) if rep.edits.is_empty() => run
                            .summary
                            .push_str(&format!("arts-power: {tok} unchanged\n")),
                        Ok(rep) => {
                            for e in &rep.edits {
                                let combo: String = e
                                    .combo
                                    .iter()
                                    .map(legaia_arts_patch::arts_power::command_glyph)
                                    .collect();
                                run.summary.push_str(&format!(
                                    "arts-power: {combo} ({:?}) -> {value:#04X}\n",
                                    e.character
                                ));
                            }
                        }
                        Err(e) => run.summary.push_str(&format!("arts-power: {e}\n")),
                    }
                }
                None => run
                    .summary
                    .push_str(&format!("arts-power: skipped malformed entry {tok:?}\n")),
            }
        }
    }
    Ok(())
}

/// Super Art damage-power edits: `NAME=VALUE` tokens
/// (`Tri-Somersault=0x1A`). Super Art names contain spaces, so this field
/// splits on commas / semicolons / newlines only - never on a space. A Super
/// Art has no combo and no arts-name-table row, so it is addressed by name
/// and located through its finisher action constant in the character's own
/// `record0` art block. A bad entry is reported and skipped.
pub(super) fn super_art_power(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        super_art_powers, ..
    } = *run.opts;
    let super_art_powers = super_art_powers.trim();
    if super_art_powers.is_empty() {
        run.summary.push_str("super-art-power: untouched\n");
    } else {
        for tok in super_art_powers
            .split([',', ';', '\n'])
            .map(str::trim)
            .filter(|t| !t.is_empty())
        {
            let parsed = tok.split_once('=').and_then(|(n, v)| {
                let hits = legaia_arts_patch::super_art_power::find_super_art(n.trim(), None);
                let art = (hits.len() == 1).then(|| hits[0])?;
                let vs = v.trim();
                let value = vs
                    .strip_prefix("0x")
                    .or_else(|| vs.strip_prefix("0X"))
                    .map(|h| u8::from_str_radix(h, 16))
                    .unwrap_or_else(|| vs.parse::<u8>())
                    .ok()?;
                legaia_arts_patch::super_art_power::is_accepted_power(value).then_some((art, value))
            });
            match parsed {
                Some((art, value)) => {
                    match apply::set_super_art_power(&mut run.patcher, &[(art, value)]) {
                        Ok(rep) if rep.edits.is_empty() => run
                            .summary
                            .push_str(&format!("super-art-power: {} unchanged\n", art.name)),
                        Ok(rep) => {
                            for e in &rep.edits {
                                run.summary.push_str(&format!(
                                    "super-art-power: {} ({:?}) -> {value:#04X}\n",
                                    e.name, e.character
                                ));
                            }
                        }
                        Err(e) => run.summary.push_str(&format!("super-art-power: {e}\n")),
                    }
                }
                None => run.summary.push_str(&format!(
                    "super-art-power: skipped malformed entry {tok:?}\n"
                )),
            }
        }
    }
    Ok(())
}

/// Arts AP override: comma/space/newline-separated `[CHARACTER:]COMBO=AMOUNT`
/// tokens (`Vahn:RDLDL=10`). In `arts_ap_grants` the amount is AP granted per
/// use (the art becomes castable at any AP level and adds that much, clamped
/// at 100); in `arts_ap_costs` it is the flat AP the art charges, replacing
/// retail's computed `multiplier x command count`. Each entry lands in its own
/// per-(character, row) config cell, so one character's art never moves
/// another's. Mutually exclusive with shiny-seru (guarded above).
pub(super) fn arts_ap_override(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        arts_ap_grants,
        arts_ap_costs,
        ..
    } = *run.opts;
    let arts_ap_grants = arts_ap_grants.trim();
    let arts_ap_costs = arts_ap_costs.trim();
    if arts_ap_grants.is_empty() && arts_ap_costs.is_empty() {
        run.summary.push_str("arts-ap: untouched\n");
    } else {
        let mut specs: Vec<legaia_arts_patch::arts_ap_grant::ArtApSpec> = Vec::new();
        for (src, grant) in [(arts_ap_grants, true), (arts_ap_costs, false)] {
            for tok in src
                .split([',', ';', '\n', ' '])
                .filter(|t| !t.trim().is_empty())
            {
                match parse_art_ap_token(tok, grant) {
                    Some(s) => specs.push(s),
                    None => run
                        .summary
                        .push_str(&format!("arts-ap: skipped malformed entry {tok:?}\n")),
                }
            }
        }
        if specs.is_empty() {
            run.summary.push_str("arts-ap: no valid entries\n");
        } else {
            match apply::inject_arts_ap_grant(&mut run.patcher, &specs) {
                Ok(rep) => {
                    for g in &rep.resolved {
                        let combo = legaia_arts_patch::arts_ap_grant::combo_str(&g.combo);
                        let what = if g.mode.is_grant() {
                            format!("grants {} AP", g.mode.amount())
                        } else {
                            format!("costs {} AP", g.mode.amount())
                        };
                        run.summary.push_str(&format!(
                            "arts-ap: {:?} {combo} {:?} {what} (menu list now reads {}, was {})\n",
                            g.character, g.name, g.display_ap, g.previous_display_ap
                        ));
                    }
                }
                Err(e) => run.summary.push_str(&format!("arts-ap: {e}\n")),
            }
        }
    }
    Ok(())
}

pub(super) fn arts_combos(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions { arts, .. } = *run.opts;
    let arts_mode = parse_mode(arts).map(|m| match m {
        DropMode::Shuffle => legaia_arts_patch::arts::ArtsMode::Shuffle,
        DropMode::Random => legaia_arts_patch::arts::ArtsMode::Random,
    });
    match arts_mode {
        Some(m) => {
            let (_plan, rep) = apply::randomize_arts(&mut run.patcher, run.seed, m)
                .map_err(|e| format!("arts: {e}"))?;
            run.summary.push_str(&format!(
                "arts: {} of {} arts re-combo'd ({})\n",
                rep.combos_changed, rep.arts, arts
            ));
        }
        None => run.summary.push_str("arts: untouched\n"),
    }
    Ok(())
}
