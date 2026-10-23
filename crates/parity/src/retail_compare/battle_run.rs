//! The battle-state run and its image comparison.

use super::*;

/// Prefix of the reason a battle-class state carries when its RAM does not
/// describe a seedable fight.
pub const BATTLE_NOT_SEEDABLE: &str = "battle not seedable: ";

/// The `resident_patch` tag of a state made under the shiny-Seru patch, and
/// the boost that patch applies to a shiny monster's HP / MP maxima.
const SHINY_SERU_PATCH: &str = "shiny-seru";

/// Whether a state's `resident_patch` tag names the shiny-Seru build.
pub(super) fn is_shiny_seru_patch(resident_patch: Option<&str>) -> bool {
    resident_patch.is_some_and(|p| p.contains(SHINY_SERU_PATCH))
}
const SHINY_BOOST_NUM: u32 = 135;
const SHINY_BOOST_DEN: u32 = 100;

/// The idle orbit's turn a camera pass (`-4` yaw units,
/// `BattleCamera::align_orbit_yaw`), taken back over the display lag.
const ORBIT_YAW_PER_PASS: i32 = 4;

/// `LEGAIA_RC_SEED_TRACE=1`: one stderr line per stream the search tried
/// (what it reached, and the ground drift it measured) and one for the
/// undrift re-run's verdict.
fn seed_trace() -> bool {
    std::env::var_os("LEGAIA_RC_SEED_TRACE").is_some()
}

/// `LEGAIA_RC_SEED_SCAN=1`: the stream search runs every stream instead of
/// stopping at the first that reaches the phase, so one seed trace lists
/// each stream's outcome - the input to a pin search. The first reaching
/// stream is still the one scored.
fn seed_scan() -> bool {
    std::env::var("LEGAIA_RC_SEED_SCAN").is_ok_and(|v| v.trim() == "1")
}

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
    // The flags the fight's own record raised on its way in are held back
    // from the seed's scene entry (`retail_compare_battle::entry_latches`).
    let latched;
    let battle = {
        let mut b = battle.clone();
        b.resident_shiny_seru = is_shiny_seru_patch(entry.resident_patch.as_deref());
        b.entry_latches = crate::retail_compare_battle::entry_latches(opts.extracted, retail, &b);
        latched = b;
        &latched
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
    // A party melee capture also wants retail's landed swings: the ring
    // holds one push per hit that reached the damage body, and a blocked
    // swing pushes none, so a stream whose block rolls landed another count
    // replays another string - another total, another knockback, another
    // number on screen (`battle_melee_hit_spark`: three of four landed).
    let ring_pushes = {
        let seed = battle.melee_hit_seeds();
        let party_swing = matches!(
            battle.seed_plan(),
            crate::retail_compare_battle::SeedPlan::Action { seat, .. } if seat < 3
        );
        // A capture past the end signal is placed by the results sequence,
        // not inside the string, and already searches on its win pose.
        (party_swing && battle.win_pose.is_none() && !seed.hits.is_empty())
            .then_some(seed.hits.len() as u32)
    };
    let mut off_hits = None;
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
    // A state's pinned stream first (`BATTLE_RNG_PINS`), then the list.
    // `LEGAIA_RC_PIN_SEED` stands in for the pin, for a sweep;
    // `LEGAIA_RC_NO_PINS=1` drops the table, for the sweep's unpinned pass.
    let no_pins = std::env::var("LEGAIA_RC_NO_PINS").is_ok_and(|v| v.trim() == "1");
    let pinned = std::env::var("LEGAIA_RC_PIN_SEED")
        .ok()
        .and_then(|v| u32::from_str_radix(v.trim().trim_start_matches("0x"), 16).ok())
        .or_else(|| {
            (!no_pins)
                .then(|| crate::retail_compare_battle::pinned_seed(&entry.label))
                .flatten()
        });
    let seeds: Vec<u32> = pinned
        .into_iter()
        .chain(crate::retail_compare_battle::BATTLE_RNG_SEEDS)
        .collect();
    'search: for &victims in revive {
        for &seed in &seeds {
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
                aged.span_gate = legaia_engine_capture::battle_drive::SpanGate::Age { accum };
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
            if seed_trace() {
                match &run {
                    Ok(e) => eprintln!(
                        "[seed] {} seed={seed:#010X} victims={victims} end={:#010X} mode={:?} prompt={:?} \
                         surprise={} driven={:?} inflight={:?} age_short={:?} drift={:?}",
                        entry.label,
                        e.rng_at_end,
                        e.mode,
                        e.prompt_tick,
                        e.surprise_opening,
                        e.driven,
                        e.inflight,
                        e.age_short,
                        e.ground_drift,
                    ),
                    Err(err) => {
                        eprintln!("[seed] {} seed={seed:#010X} failed: {err:#}", entry.label)
                    }
                }
            }
            match run {
                Ok(e)
                    if e.mode == legaia_engine_mode::mode::SceneMode::Battle
                        && e.prompt_tick.is_some()
                        && (!e.surprise_opening || opening)
                        && e.driven != Some(None)
                        && e.inflight != Some(None) =>
                {
                    if battle.win_pose.is_some() && e.win_pose != battle.win_pose {
                        off_pose.get_or_insert(e);
                        continue;
                    }
                    if ring_pushes.is_some() && e.action_pushes != ring_pushes {
                        off_hits.get_or_insert((e, victims));
                        continue;
                    }
                    if reached.is_none() {
                        reached = Some(e);
                        reached_victims = victims;
                    }
                    if !seed_scan() {
                        break 'search;
                    }
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
    // No stream landed retail's count: the first that reached the phase is
    // kept, as it was before the count was asked for.
    if reached.is_none()
        && let Some((e, victims)) = off_hits.take()
    {
        reached = Some(e);
        reached_victims = victims;
    }
    // A driven action replays the pushes its capture already holds; run the
    // same stream once more from the ground the replay says retail started
    // on (`RetailBattle::undrift`), under each rule for the acting seat, and
    // keep the run that still reaches the phase and stands the combatants
    // nearest retail's pairs - a replay whose push depends on where it
    // starts can land further off than the first run did.
    let undrifted;
    let (battle, reached) = match reached {
        Some(first_run) => {
            let seed = first_run.rng_seed;
            let mut best_residual =
                crate::retail_compare_battle::ground_residual(battle, battle, &first_run);
            let mut best: Option<(crate::retail_compare_battle::RetailBattle, _)> = None;
            let mut tried: Vec<crate::retail_compare_battle::RetailBattle> = Vec::new();
            for rule in crate::retail_compare_battle::UndriftActing::ALL {
                // A replayed push depends on where it starts, so one step
                // back by the measured drift can still stand a combatant
                // short: the seat's own rule is iterated on each re-run's
                // drift (`captured - drift` again) while its residual falls.
                let mut drift = first_run.ground_drift;
                let mut rule_best: Option<(crate::retail_compare_battle::RetailBattle, _, _)> =
                    None;
                for _ in 0..rule.rounds() {
                    let Some(b) = battle.undrift(&drift, rule) else {
                        break;
                    };
                    if tried.iter().any(|t| t.seeded_ground() == b.seeded_ground()) {
                        break;
                    }
                    let run = crate::retail_compare_battle::run_engine_battle(
                        opts.extracted,
                        retail,
                        &b,
                        seed,
                        reached_victims,
                    );
                    tried.push(b.clone());
                    let Ok(e) = run else {
                        break;
                    };
                    let residual = crate::retail_compare_battle::ground_residual(battle, &b, &e);
                    if seed_trace() {
                        eprintln!(
                            "[seed] {} undrift ({rule:?}) seed={seed:#010X} from {:?}: mode={:?} \
                             driven={:?} age_short={:?} drift={:?} residual {residual:?} against \
                             the best so far {best_residual:?}",
                            entry.label,
                            b.seeded_ground(),
                            e.mode,
                            e.driven,
                            e.age_short,
                            e.ground_drift,
                        );
                    }
                    let reaches = e.mode == legaia_engine_mode::mode::SceneMode::Battle
                        && e.driven.is_some_and(|d| d.is_some())
                        && e.age_short.is_none();
                    if !reaches {
                        break;
                    }
                    // A worse round is not the end of the iteration: the
                    // push a replay lands is not monotone in where it
                    // starts, so the next round, taken on this one's drift,
                    // can land nearer than any before it.
                    drift = e.ground_drift;
                    if !rule_best.as_ref().is_some_and(|(_, _, r)| residual >= *r) {
                        rule_best = Some((b, e, residual));
                    }
                }
                // The first rule has only to beat the first run. A later one
                // replaces a rule that already stands the fight nearer only
                // by a margin a framing can read (`UNDRIFT_MIN`): a few
                // units of residual either way is two replays of one push,
                // not a nearer ground.
                if let Some((b, e, residual)) = rule_best {
                    let nearer = match (best.is_some(), residual, best_residual) {
                        (true, Some(r), Some(have)) => {
                            r + i64::from(crate::retail_compare_battle::UNDRIFT_MIN) <= have
                        }
                        _ => residual < best_residual,
                    };
                    if nearer {
                        best_residual = residual;
                        best = Some((b, e));
                    }
                }
            }
            match best {
                Some((b, e)) => {
                    undrifted = b;
                    (&undrifted, Some(e))
                }
                None => (battle, Some(first_run)),
            }
        }
        None => (battle, None),
    };
    // A replayed cast reaches its phase with every bystander's idle loop on
    // the replay's own clock, and the sway moves the body pairs the
    // separation pass measures through the whole cast: run the same stream
    // once more with each loop wound back to stand on the capture's cursor
    // at the phase (`RetailBattle::with_inflight_idle`), and keep it when it
    // still reaches the phase.
    let idled;
    let (battle, reached) = match reached {
        Some(first_run) if first_run.inflight.is_some_and(|t| t.is_some()) => {
            match battle.with_inflight_idle(&first_run.idle_advance) {
                Some(b) => match crate::retail_compare_battle::run_engine_battle(
                    opts.extracted,
                    retail,
                    &b,
                    first_run.rng_seed,
                    reached_victims,
                ) {
                    Ok(e)
                        if e.mode == legaia_engine_mode::mode::SceneMode::Battle
                            && e.inflight.is_some_and(|t| t.is_some()) =>
                    {
                        idled = b;
                        (&idled, Some(e))
                    }
                    _ => (battle, Some(first_run)),
                },
                None => (battle, Some(first_run)),
            }
        }
        r => (battle, r),
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
            b.span_gate = legaia_engine_capture::battle_drive::SpanGate::Age { accum };
            aged = b;
            &aged
        }
        None => battle,
    };
    let image = battle_image(opts, entry, retail, battle, &engine, report);
    // The shiny-Seru patch boosts a shiny monster's maxima at battle init
    // (`x135/100`, truncated). A maximum that reads exactly that over the
    // engine's disc value is the patch's write, not retail's, and is left
    // unscored like a probe's; any other difference still scores.
    let mut unscored = entry.ram_injected.clone();
    if is_shiny_seru_patch(entry.resident_patch.as_deref()) {
        for (m, (r, e)) in battle.monsters.iter().zip(&engine.monsters).enumerate() {
            let Some(r) = r else { continue };
            let boosted = |v: u16| (u32::from(v) * SHINY_BOOST_NUM / SHINY_BOOST_DEN) as u16;
            for (name, want, got) in [
                ("hp_max", r.hp_max, e.hp_max),
                ("mp_max", r.mp_max, e.mp_max),
            ] {
                if want != got && want == boosted(got) {
                    unscored.push(format!("m{m}.{name}"));
                }
            }
        }
    }
    let (mut ch, mut det) =
        crate::retail_compare_battle::compare_battle(retail, battle, &engine, &unscored);
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
    // reads it says so instead of reading as an engine miss. So does the
    // frame: a party-swap patch draws its own battle models (the Delilas
    // captures' fighter where the engine draws the unpatched member).
    if let Some(patch) = entry.resident_patch.as_deref() {
        for key in ["enemy_hp", "battle_party", "image"] {
            if let Some(d) = det.get_mut(key) {
                d.push_str(&format!(
                    "; retail ran a patched executable (resident patch: {patch})"
                ));
            }
        }
    }
    report.image = image;
    report.detail.extend(det);
    report
        .detail
        .insert("stream".into(), format!("{:#010X}", engine.rng_seed));
    report.score = state_score(&ch);
    report.channels = ch;
}

/// The battle frame through `play-window --battle` (the fight's MAN row, or
/// its formation cell where no row carries it), when the retail frame is not
/// a fade.
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
    let flags = retail
        .save
        .as_ref()
        .map(system_flag_ids)
        .unwrap_or_default();
    let extra = crate::retail_compare_battle::play_window_args(battle, engine.man_row);
    // The child lands the save the headless seed landed: the fight's own
    // entry latches held back, and raised after its settle
    // (`LEGAIA_BATTLE_LATCHES`).
    let child_save = retail.save.clone().map(|mut save| {
        crate::retail_compare::clear_system_flags(&mut save, &battle.entry_latches);
        save
    });
    let mut env = vec![
        (
            "LEGAIA_BATTLE_STAGE",
            format!(
                "{},{}",
                battle.stage_variant,
                u8::from(battle.keep_backdrop_object_1)
            ),
        ),
        // The slot-1 backdrop angle the displayed frame was drawn at: the
        // RAM's, less the two frames of winding the display lags it by.
        (
            "LEGAIA_BATTLE_BACKDROP_YAW",
            battle.displayed_backdrop_slot_1_yaw().to_string(),
        ),
        ("LEGAIA_BATTLE_RNG_SEED", engine.rng_seed.to_string()),
        // The headless seed settles the landed field before it seeds the
        // stream and arms the fight (`run_engine_battle`); so does the child.
        ("LEGAIA_BATTLE_SETTLE", SETTLE_TICKS.to_string()),
        // The mid-fight bars the headless seed put on its first battle tick.
        (
            "LEGAIA_BATTLE_BARS",
            legaia_engine_capture::bar_seeds::bar_seeds_to_env(&engine.hp_seed),
        ),
    ];
    // A shiny-Seru build's entry draw, which the child's `--retail` pin would
    // otherwise drop while the headless seed took it
    // (`RetailBattle::resident_shiny_seru`).
    if battle.resident_shiny_seru {
        env.push(("LEGAIA_BATTLE_SHINY_SERU", "1".to_string()));
    }
    // Each body on the keyframe the capture shows it at.
    let seeds = battle.engine_clip_cursors();
    if !seeds.is_empty() {
        env.push((
            "LEGAIA_SEAT_BATTLE_CLIPS",
            legaia_engine_capture::clip_cursors::clip_cursors_to_env(&seeds),
        ));
    }
    // A party melee capture's landed hits, so the replayed combo deals
    // retail's amounts (`legaia_engine_capture::melee_hits`).
    let hits = battle.melee_hit_seeds();
    if !hits.is_empty() {
        env.push((
            "LEGAIA_BATTLE_MELEE_HITS",
            legaia_engine_capture::melee_hits::melee_hits_to_env(&hits),
        ));
    }
    if battle.mystic_shield {
        env.push(("LEGAIA_BATTLE_MYSTIC_SHIELD", "1".to_string()));
    }
    // The cast's live parts where retail's passes had put them: a part
    // rolled off `rand()` or aged by the frame-skip history stands where no
    // replay reproduces.
    if !battle.cast_parts.is_empty() {
        env.push((
            "LEGAIA_SEAT_CAST_PARTS",
            legaia_engine_capture::cast_parts::cast_parts_env(&battle.cast_parts),
        ));
    }
    // ... and the effect-script table parts' rotation clocks, which carry
    // the frame step of the pass that seated them.
    if !battle.action_fx_parts.is_empty() {
        env.push((
            "LEGAIA_SEAT_ACTION_FX_PARTS",
            legaia_engine_capture::cast_parts::cast_parts_env(&battle.action_fx_parts),
        ));
    }
    // The effect pool's billboards as the displayed frame held them: where a
    // burning body's sheds and a master's randomized legs put each child is
    // the stream's history. An empty pool is seated too, which retires the
    // billboards a replay running behind retail's clock still carries.
    if let Some(children) = &battle.effect_children {
        env.push((
            "LEGAIA_SEAT_EFFECT_CHILDREN",
            legaia_engine_capture::effect_children::effect_children_env(children),
        ));
    }
    // ... and the after-image ghosts at the keyframes and ground the
    // capture's history ring held: the replay's ring runs on its own frame
    // step, so the walk's depths reach other keyframes of the swing.
    if !battle.ghost_ring.is_empty() {
        env.push((
            "LEGAIA_SEAT_GHOST_RING",
            legaia_engine_capture::ghost_ring::ghost_ring_to_env(&battle.ghost_ring),
        ));
    }
    // The live effect scrollers' rects as the displayed frame held them: a
    // scroller's rotation count is time since its effect was seated.
    if let Some(kv) = retail.vram_rects_env(opts.out_dir, &entry.label) {
        env.push(kv);
    }
    if !battle.entry_latches.is_empty() {
        env.push((
            "LEGAIA_BATTLE_LATCHES",
            battle
                .entry_latches
                .iter()
                .map(|f| format!("{f:x}"))
                .collect::<Vec<_>>()
                .join(","),
        ));
    }
    // The idle orbit is a clock: phase-align it to the retail instant when
    // retail's own orbit owns the yaw (the battle tick's prologue store,
    // gated on these command-flow bytes - `0x801D07AC..0x801D07CC`).
    // The frame on screen is two passes older, and the orbit turns `-4` a
    // pass (`v0_1_battle_command_menu`: the RAM's yaw reads `.976`, eight
    // units back `1.000`).
    if battle.orbit_owns_yaw() {
        let back = i32::from(crate::retail_compare_battle::DISPLAY_LAG_PASSES);
        let yaw = (i32::from(retail.camera.yaw) + ORBIT_YAW_PER_PASS * back).rem_euclid(4096);
        env.push(("LEGAIA_BATTLE_ORBIT_YAW", yaw.to_string()));
    }
    // The captured phase's glide starts where retail's stood, as the
    // headless channel aligns it (`BattleCamera::align_glide_origin`).
    // A replayed cast in the invoke `0x32`: the frame on screen is two game
    // frames back along retail's ease (`RetailBattle::cast_glide_aligned`).
    if battle.cast_glide_aligned() {
        env.push((
            "LEGAIA_BATTLE_CAM_ALIGN",
            legaia_engine_capture::camera_align::cam_align_to_env(&(
                battle.displayed_cam_live,
                battle.cam_tween.1,
            )),
        ));
    }
    // A replayed cast inside a module shot still walking: the frame on
    // screen two passes back along the shot's own table
    // (`RetailBattle::module_cam_aligned`).
    if battle.module_cam_aligned() {
        env.push((
            "LEGAIA_BATTLE_MODULE_CAM_ALIGN",
            legaia_engine_capture::camera_align::cam_align_to_env(&(
                battle.displayed_module_cam_live,
                battle.cam_tween.1,
            )),
        ));
    }
    // Every pad drive, as the headless seed snaps and aligns every driven
    // capture: a menu capture's case-0 close-up subtracts its member's
    // heading (`0x8F0 - actor[+0x46]`), which only the ground snap carries -
    // the replay never ran the earlier turn that left it.
    if matches!(
        battle.battle_drive(),
        Some(
            legaia_engine_capture::battle_drive::BattleDrive::Action { .. }
                | legaia_engine_capture::battle_drive::BattleDrive::Menu { .. }
        )
    ) {
        env.push((
            "LEGAIA_BATTLE_CAM_ALIGN",
            legaia_engine_capture::camera_align::cam_align_to_env(&if battle.displayed_camera() {
                (battle.displayed_cam_live, battle.cam_tween.1)
            } else {
                battle.cam_tween
            }),
        ));
        env.push((
            "LEGAIA_BATTLE_GROUND_SNAP",
            legaia_engine_capture::ground_snap::ground_snaps_to_env(
                &if battle.displayed_camera() {
                    battle.engine_image_ground_snaps()
                } else {
                    battle.engine_ground_snaps()
                },
            ),
        ));
        if battle.displayed_camera_on_agreement() {
            env.push((
                "LEGAIA_BATTLE_CAM_ALIGN_DISPLAYED",
                legaia_engine_capture::camera_align::cam_align_to_env(&(
                    battle.displayed_cam_live,
                    battle.cam_tween.1,
                )),
            ));
        }
    }
    env.push((
        "LEGAIA_BATTLE_CAMERA_OPTION",
        battle.camera_option.to_string(),
    ));
    // Retail's HUD glides had all landed: the frame shows every plate at its
    // rest seat whatever the replay's own seed-to-phase time was (a seat
    // seeded on its captured ground skips the approach retail spent the
    // sixteen-frame raise on).
    // A menu capture whose displayed frame predates every glide in flight
    // shows the surface before them, which the player sat on: landed.
    let displayed_menu = battle.displayed_menu_flow().is_some();
    if battle.hud_glides_landed || displayed_menu {
        env.push(("LEGAIA_SEAT_HUD_GLIDES_LANDED", "1".to_string()));
    }
    // Glides still in flight: each seated on the elapsed the displayed
    // frame shows (`HudGlideSeat`).
    if !battle.hud_glides.is_empty() && !displayed_menu {
        env.push((
            "LEGAIA_SEAT_HUD_GLIDES",
            legaia_engine_capture::hud_glide::HudGlideSeat::to_env(&battle.hud_glides),
        ));
    }
    // The numerals the displayed frame drew, off retail's own packets
    // (`displayed_popup_cells`); where they cannot be read, the newest
    // numeral's rise (`popup_age_seat`) on the engine seat of its struck
    // pool slot.
    if let Some(cells) = battle.popup_cells.as_ref() {
        env.push((
            "LEGAIA_SEAT_POPUP_CELLS",
            crate::retail_compare_battle::popup_cells_to_env(cells),
        ));
    }
    if let Some((slot, age)) = battle.popup_age.filter(|_| battle.popup_cells.is_none()) {
        let seat =
            legaia_engine_capture::battle_drive::engine_seat(slot, battle.party_count.clamp(1, 3));
        env.push(("LEGAIA_SEAT_POPUP_AGE", format!("{seat}:{age}")));
    }
    // A Spirit turn's bar and plate, sized as retail sized them.
    if let Some((width, plate)) = battle.spirit_gauge {
        env.push(("LEGAIA_SEAT_SPIRIT_GAUGE", format!("{width},{plate}")));
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
        let idle: Vec<String> = seed
            .idle
            .iter()
            .map(|c| c.map_or_else(|| "-".to_string(), |c| c.to_string()))
            .collect();
        env.push((
            "LEGAIA_BATTLE_INFLIGHT",
            format!(
                "{},{},{};{};{}",
                seed.caster,
                seed.spell_id,
                seed.target,
                ground.join(","),
                idle.join(",")
            ),
        ));
        // On the battle-frame cadence a `0x32` capture's displayed frame is
        // the pass before the state was entered (`0x29`'s summon-route pass):
        // the RAM gate holds one whole pass in `0x32`, and the display is two
        // passes older. A gate on the state cannot name it, so the child
        // takes the RAM's gate and re-runs to the tick the display lag back
        // (`LEGAIA_CAPTURE_DISPLAY_LAG`, the retake).
        let ram_gate = battle.phase_gate();
        match ram_gate {
            Some(g)
                if g.camera_pass
                    && battle.display_lag > 0
                    && legaia_engine_session::native::battle_frame_cadence_on() =>
            {
                env.push(("LEGAIA_CAPTURE_GATE", g.to_env()));
                env.push(("LEGAIA_CAPTURE_DISPLAY_LAG", battle.display_lag.to_string()));
            }
            _ => env.push(("LEGAIA_CAPTURE_GATE", gate.to_env())),
        }
        tick += crate::retail_compare_battle::INFLIGHT_DEADLINE;
    }
    // A menu capture or any other action in flight is walked there through
    // the pad path, the same drive the headless seed ran, and captured the
    // first frame it holds.
    if let Some(drive) = battle.image_drive() {
        if engine.driven == Some(None) {
            report.detail.insert(
                "image".into(),
                "not scored: the pad drive never reached the capture's phase headlessly".into(),
            );
            return None;
        }
        env.push(("LEGAIA_BATTLE_DRIVE", drive.to_env()));
        // The display shows the frame two game frames older than the RAM;
        // where it measures better the child re-runs to it
        // (`RetailBattle::display_retake`).
        if battle.display_retake() {
            env.push(("LEGAIA_CAPTURE_DISPLAY_LAG", battle.display_lag.to_string()));
        }
        tick += legaia_engine_capture::battle_drive::DRIVE_DEADLINE;
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
        match child_save.as_ref() {
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

#[cfg(test)]
mod shiny_patch_tests {
    use super::is_shiny_seru_patch;

    /// The corpus's shiny states carry the build's layout in the tag; a
    /// party-swap or unpatched state takes retail's entry with no shiny draw.
    #[test]
    fn shiny_seru_tag_selects_the_entry_draw() {
        assert!(is_shiny_seru_patch(Some(
            "shiny-seru (earlier SCUS layout)"
        )));
        assert!(is_shiny_seru_patch(Some("shiny-seru")));
        assert!(!is_shiny_seru_patch(Some("party swap")));
        assert!(!is_shiny_seru_patch(None));
    }
}
