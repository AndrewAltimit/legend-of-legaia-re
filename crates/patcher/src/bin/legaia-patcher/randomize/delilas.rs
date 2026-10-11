//! The two Delilas features: the dome course and the party swap.

use anyhow::Result;

use legaia_patcher::apply;

use super::Run;

/// Delilas Challenge: a fourth Muscle Dome enrollment option - a 2-round
/// arena course, Che & Lu double-team (1v2) then Gi (1v1), routed through
/// the real arena (magic-off), gated on the Koru event (story flag 0x378).
/// The 1v2 fits the battle heap via slim clones at unreachable archive
/// slots + a stream-map hook; the originals are untouched. Losing a round
/// returns to the venue by the dome's design; a full clear pays 5000
/// coins. koin1 script + a companion arena/SCUS code injection.
pub(super) fn delilas_challenge(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    if args.delilas_challenge {
        match apply::apply_delilas_challenge(patcher, args.custom_items) {
            Ok(report) if report.changed => {
                println!(
                    "delilas-challenge: Muscle Dome enrollment offers the Delilas Challenge \
                     (a 2-round dome course: Che & Lu double-team, then Gi; a full clear \
                     pays 5000 coins + {}; unlocks after Nivora Ravine; {} bytes of new \
                     koin1 script in PROT {}{})",
                    if args.custom_items {
                        "the three custom items"
                    } else {
                        "a Honey"
                    },
                    report.grown_bytes,
                    report.entry_idx,
                    if report.dome_injected {
                        " + arena course injected"
                    } else {
                        ""
                    }
                );
                manifest.push("delilas_challenge = true".to_string());
                manifest.push(format!(
                    "delilas_reward_custom_items = {}",
                    args.custom_items
                ));
            }
            Ok(_) => {
                println!("delilas-challenge: already applied");
                manifest.push("delilas_challenge = true".to_string());
                manifest.push(format!(
                    "delilas_reward_custom_items = {}",
                    args.custom_items
                ));
            }
            Err(e) => {
                // A grown koin1 MAN (e.g. a language pack) can leave no room;
                // skip rather than abort the whole run.
                println!("delilas-challenge: skipped ({e:#})");
                manifest.push("delilas_challenge = skipped".to_string());
            }
        }
    } else {
        manifest.push("delilas_challenge = false".to_string());
    }
    Ok(())
}

/// Delilas party swap: play as the siblings, fight the heroes. Never
/// runs alongside --delilas-challenge (refused above).
pub(super) fn delilas_party(run: &mut Run<'_>) -> Result<()> {
    let args = run.args;
    let patcher = &mut run.patcher;
    let manifest = &mut run.manifest;
    let arts_ap = !args.arts_ap_grant.is_empty() || !args.arts_ap_cost.is_empty();
    let oscillating = args.oscillating_ap.is_some();
    if let Some(mapping) = &args.delilas_party {
        let cast_route = if args.shiny_seru
            || args.show_super_arts
            || args.super_arts_pack
            || arts_ap
            || oscillating
        {
            legaia_delilas_party::delilas_party::CastRoutePolicy::ArenaTaken
        } else {
            legaia_delilas_party::delilas_party::CastRoutePolicy::Install
        };
        match legaia_delilas_party::delilas_party::apply_delilas_party_with(
            patcher,
            mapping,
            args.delilas_arts_voice,
            args.delilas_moves,
            cast_route,
            legaia_delilas_party::delilas_party::DelilasPartyOptions {
                keep_che_hammer: args.delilas_che_hammer,
            },
        ) {
            Ok(report) if report.changed => {
                println!(
                    "delilas-party: playing as {} (Vahn), {} (Noa), {} (Gala); the ravine \
                     duels + dome Master legs field Vahn / Noa / Gala models instead",
                    mapping.vahn.display_name(),
                    mapping.noa.display_name(),
                    mapping.gala.display_name(),
                );
                for note in &report.notes {
                    println!("  {note}");
                }
                manifest.push(format!(
                    "delilas_party = {},{},{}",
                    mapping.vahn.display_name().to_ascii_lowercase(),
                    mapping.noa.display_name().to_ascii_lowercase(),
                    mapping.gala.display_name().to_ascii_lowercase(),
                ));
                manifest.push(format!("delilas_arts_voice = {}", args.delilas_arts_voice));
                manifest.push(format!("delilas_moves = {}", args.delilas_moves));
                if args.delilas_che_hammer {
                    manifest.push("delilas_che_hammer = true".to_string());
                }
            }
            Ok(_) => {
                println!("delilas-party: already applied");
                manifest.push("delilas_party = already".to_string());
            }
            Err(e) => {
                println!("delilas-party: skipped ({e:#})");
                manifest.push("delilas_party = skipped".to_string());
            }
        }
    } else {
        manifest.push("delilas_party = false".to_string());
    }
    Ok(())
}
