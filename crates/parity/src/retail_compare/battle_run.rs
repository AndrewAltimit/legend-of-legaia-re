//! The battle-state run and its image comparison. Split out of
//! `retail_compare.rs`; no logic change.

use super::*;

/// Prefix of the reason a battle-class state carries when its RAM does not
/// describe a seedable fight.
pub const BATTLE_NOT_SEEDABLE: &str = "battle not seedable: ";

pub(super) fn run_battle(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    report: &mut StateReport,
) {
    let battle = match &retail.battle {
        Some(Ok(b)) => b,
        Some(Err(why)) => {
            report.unseeded = format!("{BATTLE_NOT_SEEDABLE}{why}");
            return;
        }
        None => {
            report.unseeded = "battle observables not read".into();
            return;
        }
    };
    // The first stream under which the fight is still on at the sample, its
    // opening reached a prompt without a surprise round the capture's own
    // history does not hold (`EngineBattle::surprise_opening`; an opening
    // capture is that round), and the drive / replayed cast reached the
    // capture's phase (`BATTLE_RNG_SEEDS`), first with the HP as read and
    // then with a party swing's victims revived
    // (`RetailBattle::action_victims`); a state nothing satisfies keeps the
    // first run.
    let mut first = None;
    let mut reached = None;
    let mut reached_victims = false;
    // A battle-end capture also wants retail's win pose: a run that reaches
    // the phase on another one is kept as the fallback while the remaining
    // streams are tried (`RetailBattle::win_pose`).
    let mut off_pose = None;
    let opening = battle.seed_plan() == crate::retail_compare_battle::SeedPlan::Opening;
    // A replayed cast tries its victims revived first: unlike a pad-driven
    // swing, a cast onto a corpse still reaches its phase, so the HP-as-read
    // run would always win and the kill would never be replayed.
    let revive: &[bool] = if battle.action_victims().is_empty() {
        &[false]
    } else if battle.seed_plan() == crate::retail_compare_battle::SeedPlan::Cast {
        &[true, false]
    } else {
        &[false, true]
    };
    'search: for &victims in revive {
        for &seed in &crate::retail_compare_battle::BATTLE_RNG_SEEDS {
            let mut run = crate::retail_compare_battle::run_engine_battle(
                opts.extracted,
                retail,
                battle,
                seed,
                victims,
            );
            // An aged action state the engine left younger than retail's:
            // the same stream again, sampled on that state's last tick.
            if let Ok(e) = &run
                && let Some(accum) = e.age_short
            {
                let mut aged = battle.clone();
                aged.span_gate = crate::retail_compare_battle::SpanGate::Age { accum };
                run = crate::retail_compare_battle::run_engine_battle(
                    opts.extracted,
                    retail,
                    &aged,
                    seed,
                    victims,
                )
                .map(|mut e| {
                    e.age_short = Some(accum);
                    e
                });
            }
            match run {
                Ok(e)
                    if e.mode == legaia_engine_core::world::SceneMode::Battle
                        && e.prompt_tick.is_some()
                        && (!e.surprise_opening || opening)
                        && e.driven != Some(None)
                        && e.inflight != Some(None) =>
                {
                    if battle.win_pose.is_some() && e.win_pose != battle.win_pose {
                        off_pose.get_or_insert(e);
                        continue;
                    }
                    reached = Some(e);
                    reached_victims = victims;
                    break 'search;
                }
                Ok(e) => {
                    first.get_or_insert(Ok(e));
                }
                Err(e) => {
                    first.get_or_insert(Err(e));
                    break 'search;
                }
            }
        }
    }
    // A driven action replays the pushes its capture already holds; run the
    // same stream once more from the ground the replay says retail started
    // on (`RetailBattle::undrift`), and keep it when it still reaches the
    // phase.
    let undrifted;
    let (battle, reached) = match reached
        .as_ref()
        .and_then(|e| battle.undrift(&e.ground_drift).map(|b| (b, e.rng_seed)))
    {
        Some((b, seed)) => match crate::retail_compare_battle::run_engine_battle(
            opts.extracted,
            retail,
            &b,
            seed,
            reached_victims,
        ) {
            // Kept only when it stands the combatants nearer retail's pairs
            // than the first run did - a replay whose push depends on where
            // it starts can land further off.
            Ok(e)
                if e.mode == legaia_engine_core::world::SceneMode::Battle
                    && e.driven.is_some_and(|d| d.is_some())
                    && e.age_short.is_none()
                    && crate::retail_compare_battle::ground_residual(battle, &b, &e)
                        < reached.as_ref().and_then(|r| {
                            crate::retail_compare_battle::ground_residual(battle, battle, r)
                        }) =>
            {
                undrifted = b;
                (&undrifted, Some(e))
            }
            _ => (battle, reached),
        },
        None => (battle, reached),
    };
    let engine = match reached.or(off_pose).map(Ok).or(first) {
        Some(Ok(e)) => e,
        Some(Err(e)) => {
            report.unseeded = format!("seeding failed: {e:#}");
            return;
        }
        None => unreachable!("BATTLE_RNG_SEEDS is not empty"),
    };
    // A re-run sampled on a shorter age: the image child walks the same
    // drive, and the detail names the gate the run used.
    let aged;
    let battle = match engine.age_short {
        Some(accum) => {
            let mut b = battle.clone();
            b.span_gate = crate::retail_compare_battle::SpanGate::Age { accum };
            aged = b;
            &aged
        }
        None => battle,
    };
    let image = battle_image(opts, entry, retail, battle, &engine, report);
    let (mut ch, mut det) =
        crate::retail_compare_battle::compare_battle(retail, battle, &engine, &entry.ram_injected);
    if let Some(img) = &image {
        ch.insert("image".into(), round3(img.within));
        det.insert(
            "image".into(),
            format!("mae={:.1} within={:.3} ({})", img.mae, img.within, img.note),
        );
    }
    // A state made on a patched disc replays that build's executable: what
    // the patch writes into a combatant (the shiny-Seru boost's `x135/100`
    // on a monster's maxima) is not retail behaviour, and a channel that
    // reads it says so instead of reading as an engine miss.
    if let Some(patch) = entry.resident_patch.as_deref() {
        for key in ["enemy_hp", "battle_party"] {
            if let Some(d) = det.get_mut(key) {
                d.push_str(&format!(
                    "; retail ran a patched executable (resident patch: {patch})"
                ));
            }
        }
    }
    report.image = image;
    report.detail.extend(det);
    report.score = state_score(&ch);
    report.channels = ch;
}

/// The battle frame through `play-window --battle`, when the fight has a MAN
/// row to name and the retail frame is not a fade.
pub(super) fn battle_image(
    opts: &RunOptions<'_>,
    entry: &CorpusEntry,
    retail: &RetailObs,
    battle: &crate::retail_compare_battle::RetailBattle,
    engine: &crate::retail_compare_battle::EngineBattle,
    report: &mut StateReport,
) -> Option<ImageScore> {
    let (exe, rf) = (opts.engine_exe?, retail.frame.as_ref()?);
    if crate::retail_compare_image::luma(rf) < crate::retail_compare_image::DARK_LUMA {
        report.detail.insert(
            "image".into(),
            "not scored: retail frame is a fade (near-black)".into(),
        );
        return None;
    }
    let Some(row) = engine.man_row else {
        report.detail.insert(
            "image".into(),
            "not scored: formation has no MAN row for `play-window --battle`".into(),
        );
        return None;
    };
    let flags = retail
        .save
        .as_ref()
        .map(system_flag_ids)
        .unwrap_or_default();
    let extra = crate::retail_compare_battle::play_window_args(battle, row);
    let mut env = vec![
        (
            "LEGAIA_BATTLE_STAGE",
            format!(
                "{},{}",
                battle.stage_variant,
                u8::from(battle.keep_backdrop_object_1)
            ),
        ),
        ("LEGAIA_BATTLE_RNG_SEED", engine.rng_seed.to_string()),
        // The headless seed settles the landed field before it seeds the
        // stream and arms the fight (`run_engine_battle`); so does the child.
        ("LEGAIA_BATTLE_SETTLE", SETTLE_TICKS.to_string()),
        // The mid-fight bars the headless seed put on its first battle tick.
        (
            "LEGAIA_BATTLE_BARS",
            crate::retail_compare_battle::bar_seeds_to_env(&engine.hp_seed),
        ),
    ];
    // The idle orbit is a clock: phase-align it to the retail instant when
    // retail's own orbit owns the yaw (the battle tick's prologue store,
    // gated on these command-flow bytes - `0x801D07AC..0x801D07CC`).
    if battle.orbit_owns_yaw() {
        env.push(("LEGAIA_BATTLE_ORBIT_YAW", retail.camera.yaw.to_string()));
    }
    // The captured phase's glide starts where retail's stood, as the
    // headless channel aligns it (`BattleCamera::align_glide_origin`).
    if matches!(
        battle.battle_drive(),
        Some(crate::retail_compare_battle::BattleDrive::Action { .. })
    ) {
        env.push((
            "LEGAIA_BATTLE_CAM_ALIGN",
            crate::retail_compare_battle::cam_align_to_env(&battle.cam_tween),
        ));
    }
    env.push((
        "LEGAIA_BATTLE_CAMERA_OPTION",
        battle.camera_option.to_string(),
    ));
    // Retail's HUD glides had all landed: the frame shows every plate at its
    // rest seat whatever the replay's own seed-to-phase time was (a seat
    // seeded on its captured ground skips the approach retail spent the
    // sixteen-frame raise on).
    if battle.hud_glides_landed {
        env.push(("LEGAIA_SEAT_HUD_GLIDES_LANDED", "1".to_string()));
    }
    // Glides still in flight: each seated on the elapsed the displayed
    // frame shows (`HudGlideSeat`).
    if !battle.hud_glides.is_empty() {
        env.push((
            "LEGAIA_SEAT_HUD_GLIDES",
            crate::retail_compare_battle::HudGlideSeat::to_env(&battle.hud_glides),
        ));
    }
    // A capture taken mid-cast replays its cast and is captured on its phase
    // (the gate), with the fixed tick as the deadline.
    // The capture's own frame step over its last stretch, as the headless
    // seed ran it (`RetailBattle::frame_step_seed`).
    if let Some((step, state)) = battle.frame_step_seed() {
        env.push(("LEGAIA_BATTLE_FRAME_STEP", format!("{step},{state}")));
    }
    let mut tick = crate::retail_compare_battle::BATTLE_CAPTURE_TICK
        + u64::from(engine.prompt_tick.unwrap_or(0));
    if let (Some(seed), Some(gate)) = (battle.inflight_cast(), battle.display_phase_gate()) {
        if engine.inflight == Some(None) {
            report.detail.insert(
                "image".into(),
                "not scored: the replayed cast never reached the capture's phase headlessly".into(),
            );
            return None;
        }
        let ground: Vec<String> = seed
            .ground
            .iter()
            .map(|g| g.map_or_else(|| "-".to_string(), |[x, z]| format!("{x}:{z}")))
            .collect();
        env.push((
            "LEGAIA_BATTLE_INFLIGHT",
            format!(
                "{},{},{};{}",
                seed.caster,
                seed.spell_id,
                seed.target,
                ground.join(",")
            ),
        ));
        env.push(("LEGAIA_CAPTURE_GATE", gate.to_env()));
        tick += crate::retail_compare_battle::INFLIGHT_DEADLINE;
    }
    // A menu capture or any other action in flight is walked there through
    // the pad path, the same drive the headless seed ran, and captured the
    // first frame it holds.
    if let Some(drive) = battle.battle_drive() {
        if engine.driven == Some(None) {
            report.detail.insert(
                "image".into(),
                "not scored: the pad drive never reached the capture's phase headlessly".into(),
            );
            return None;
        }
        env.push(("LEGAIA_BATTLE_DRIVE", drive.to_env()));
        tick += crate::retail_compare_battle::DRIVE_DEADLINE;
    }
    match crate::retail_compare_image::engine_frame_with(
        exe,
        opts.extracted,
        &retail.scene,
        None,
        &extra,
        &env,
        tick,
        opts.out_dir,
        &entry.label,
        // The card-load resume the headless side seeds with, so the frame's
        // party is retail's (levels, equipment, HP / MP on the HUD, the
        // assembled battle meshes) rather than the New Game template the
        // bare door entry seeds. The door with the system flags stays the
        // fallback for a state whose save window does not lift.
        match retail.save.as_ref() {
            Some(save) => crate::retail_compare_image::FrameEntry::Resume(save),
            None => crate::retail_compare_image::FrameEntry::Door(&flags),
        },
    ) {
        Ok(ef) => {
            if let Some(dir) = opts.out_dir {
                let _ = crate::retail_compare_image::write_side_by_side(
                    &dir.join(format!("{}.png", entry.label)),
                    rf,
                    &ef,
                );
            }
            Some(crate::retail_compare_image::score(rf, &ef))
        }
        Err(e) => {
            report
                .detail
                .insert("image".into(), format!("engine frame failed: {e:#}"));
            None
        }
    }
}
