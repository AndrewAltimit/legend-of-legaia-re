//! The custom item set, the Delilas Challenge and the Delilas party swap.

use super::*;

/// Custom items: inject the standalone item set (records, effect machinery,
/// battle hooks - no Delilas dependency) and widen the random-fill pool so
/// the `random` drop/chest/steal modes can hand them out. With the Delilas
/// Challenge also on, they become the course's full-clear reward (the grant
/// half is installed with the challenge below).
pub(super) fn custom_items(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        custom_items,
        delilas_challenge,
        ..
    } = *run.opts;
    if custom_items {
        match apply::inject_custom_item_set(&mut run.patcher) {
            Ok(true) => run
                .summary
                .push_str("custom-items: injected Nature's Elixir, Ra-Seru Tear, Fury Bloom\n"),
            Ok(false) => run.summary.push_str("custom-items: already injected\n"),
            Err(e) => return Err(format!("custom-items: {e:#}")),
        }
        if run.needs_pool {
            legaia_patcher::unused::extend_pool(
                &mut run.pool,
                legaia_delilas_challenge::custom_items::CUSTOM_ITEM_IDS,
            );
        } else if !delilas_challenge {
            run.summary.push_str(
                "  note: without a random drop/chest/steal mode or the Delilas Challenge, \
                 the custom items exist but nothing hands them out\n",
            );
        }
    }
    Ok(())
}

/// Delilas Challenge: a fourth Muscle Dome enrollment option that runs a
/// brand-new 2-round dome course - Che & Lu double-team (1v2), then Gi -
/// routed through the real arena (magic-off), gated on the Koru event in
/// Nivora Ravine. The 1v2 fits the battle heap via slim clones at
/// unreachable archive slots; the originals are untouched. Losing a round
/// returns to the venue by the dome's design; a full clear pays 5000
/// coins. koin1 script edit + a companion arena/SCUS code injection;
/// seedless. A koin1 MAN another edit has already grown past its
/// zero-slack footprint skips with a note instead of failing the run.
pub(super) fn delilas_challenge(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        delilas_challenge,
        custom_items,
        ..
    } = *run.opts;
    if delilas_challenge {
        match apply::apply_delilas_challenge(&mut run.patcher, custom_items) {
            Ok(rep) if rep.changed => run.summary.push_str(&format!(
                "delilas-challenge: Muscle Dome enrollment offers the Delilas Challenge \
                 (a 2-round dome course: Che & Lu double-team, then Gi; a full clear pays \
                 5000 coins + {}; unlocks after Nivora Ravine)\n",
                if custom_items {
                    "the three custom items"
                } else {
                    "a Honey"
                }
            )),
            Ok(_) => run.summary.push_str("delilas-challenge: already applied\n"),
            Err(e) => run
                .summary
                .push_str(&format!("delilas-challenge: skipped ({e:#})\n")),
        }
    } else {
        run.summary.push_str("delilas-challenge: untouched\n");
    }
    Ok(())
}

/// Delilas party swap (empty = off): play as the mapped siblings, the
/// ravine duels field Vahn / Noa / Gala models. Never runs alongside
/// the Delilas Challenge (refused above). The single heaviest
/// stage in the whole run (player files + monster blocks + field forms
/// + XA banks), so it gets its own progress label.
pub(super) fn delilas_party(run: &mut PatchRun<'_>) -> StepResult {
    let PatchOptions {
        delilas_party,
        delilas_arts_voice,
        delilas_moves,
        shiny_seru,
        show_super_arts,
        super_arts_pack,
        arts_ap_grants,
        arts_ap_costs,
        ..
    } = *run.opts;
    let delilas_party = delilas_party.trim();
    if delilas_party.is_empty() {
        run.summary.push_str("delilas-party: off\n");
    } else {
        // Arts-voice mode for the swapped shout banks; an empty or
        // unknown value falls back to the default (original).
        let arts_voice = delilas_arts_voice
            .trim()
            .parse::<legaia_delilas_party::delilas_voice_fx::ArtsVoiceMode>()
            .unwrap_or_default();
        // Move mode for the swapped kit; an empty or unknown value falls
        // back to the retail-preserving default (hybrid).
        let move_mode = delilas_moves
            .trim()
            .parse::<legaia_delilas_party::delilas_party::DelilasMoveMode>()
            .unwrap_or_default();
        match legaia_delilas_party::delilas_party::PartyMapping::parse(delilas_party) {
            Ok(mapping) => {
                let cast_route = if shiny_seru
                    || show_super_arts
                    || super_arts_pack
                    || !arts_ap_grants.trim().is_empty()
                    || !arts_ap_costs.trim().is_empty()
                    || run.opts.oscillating()
                {
                    legaia_delilas_party::delilas_party::CastRoutePolicy::ArenaTaken
                } else {
                    legaia_delilas_party::delilas_party::CastRoutePolicy::Install
                };
                match legaia_delilas_party::delilas_party::apply_delilas_party(
                    &mut run.patcher,
                    &mapping,
                    arts_voice,
                    move_mode,
                    cast_route,
                ) {
                    Ok(rep) if rep.changed => {
                        run.summary.push_str(&format!(
                            "delilas-party: playing as {} / {} / {} (duels field the heroes); \
                             moves: {move_mode}; arts voices: {arts_voice}\n",
                            mapping.vahn.display_name(),
                            mapping.noa.display_name(),
                            mapping.gala.display_name()
                        ));
                        for note in rep.notes.iter().filter(|n| n.contains("cast route")) {
                            run.summary.push_str(&format!("  {note}\n"));
                        }
                    }
                    Ok(_) => run.summary.push_str("delilas-party: already applied\n"),
                    // A mid-apply error leaves the image partially
                    // swapped (the swap touches many entries before the
                    // failing one) - shipping that as a "skipped" note
                    // would hand the user a broken hybrid ROM. Fail the
                    // whole patch instead, like the CLI does.
                    Err(e) => {
                        return Err(format!(
                            "delilas-party failed mid-apply ({e:#}); no ROM was produced - \
                             the image would have been a partial hybrid"
                        ));
                    }
                }
            }
            Err(e) => run
                .summary
                .push_str(&format!("delilas-party: skipped - {e}\n")),
        }
    }
    Ok(())
}
